//! A deterministic **Token Bucket** rate limiter driven by an injectable
//! millisecond clock.
//!
//! The limiter is fully lazy: tokens are credited only when `try_acquire` is
//! called, and the credit is computed from the *exact* elapsed time since the
//! last call. Because the fractional remainder is carried in integer
//! micro-token units, the bucket refills **continuously and without drift**
//! regardless of how irregular the call cadence is.
//!
//! All arithmetic is saturating and bounded by `capacity`, so extreme clocks
//! (including `u64::MAX` and inverted clocks) never overflow or panic.
//!
//! ```
//! use circuit_guard::RateLimiter;
//!
//! // 10 tokens per second, capacity 10.
//! let mut rl = RateLimiter::new(10, 10);
//! assert_eq!(rl.try_acquire(0, 5), Ok(()));
//! assert_eq!(rl.try_acquire(0, 5), Ok(())); // bucket now empty
//!
//! // Empty bucket: returns the wait time in ms (1 token needs 100ms at 10/s).
//! let err = rl.try_acquire(0, 1).unwrap_err();
//! assert_eq!(err, 100);
//!
//! // After 100ms the bucket has refilled 1 token again.
//! assert_eq!(rl.try_acquire(100, 1), Ok(()));
//! ```

/// A deterministic token-bucket rate limiter.
///
/// Construct with [`RateLimiter::new`] and drive it with the injectable clock
/// API [`RateLimiter::try_acquire`]. No thread is ever blocked.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    /// Hard upper bound on the number of whole tokens held (in tokens).
    capacity: u32,
    /// Refill rate in tokens per second.
    rate_per_sec: u64,
    /// Refill rate in micro-tokens per millisecond (`rate_per_sec * 1_000_000 / 1000`),
    /// i.e. `rate_per_sec * 1_000` micro-tokens per ms.
    micro_per_ms: u64,
    /// Number of micro-tokens currently available (bounded by `capacity * 1_000_000`).
    acc_micro: u64,
    /// `now_ms` at which `acc_micro` was last credited.
    last_refill_ms: u64,
}

/// Scales one token into micro-token units (1 token = 1_000_000 micro-tokens).
const MICRO_PER_TOKEN: u64 = 1_000_000;

impl RateLimiter {
    /// Creates a rate limiter.
    ///
    /// * `rate_per_sec` — sustained refill rate in whole tokens per second.
    /// * `capacity` — maximum burst size in whole tokens. Clamped to a minimum
    ///   of `1` so the limiter never dead-locks.
    pub fn new(rate_per_sec: u64, capacity: u32) -> Self {
        let capacity = capacity.max(1);
        let micro_per_ms = rate_per_sec.saturating_mul(MICRO_PER_TOKEN / 1000);
        Self {
            capacity,
            rate_per_sec,
            micro_per_ms,
            acc_micro: (capacity as u64).saturating_mul(MICRO_PER_TOKEN),
            last_refill_ms: 0,
        }
    }

    /// Returns the configured capacity in whole tokens.
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Returns the configured sustained rate in tokens per second.
    pub fn rate_per_sec(&self) -> u64 {
        self.rate_per_sec
    }

    /// Returns the number of whole tokens currently available.
    pub fn available_tokens(&self) -> u32 {
        (self.acc_micro / MICRO_PER_TOKEN) as u32
    }

    /// Credits the bucket for the elapsed time up to `now_ms`.
    fn refill(&mut self, now_ms: u64) {
        let elapsed = now_ms.saturating_sub(self.last_refill_ms);
        self.last_refill_ms = now_ms;
        if elapsed == 0 || self.micro_per_ms == 0 {
            return;
        }
        let cap_micro = (self.capacity as u64).saturating_mul(MICRO_PER_TOKEN);
        let add = match elapsed.checked_mul(self.micro_per_ms) {
            Some(v) => v,
            // Overflow of the credit product: clamp to the hard capacity.
            None => cap_micro,
        };
        self.acc_micro = self.acc_micro.saturating_add(add).min(cap_micro);
    }

    /// Tries to acquire `tokens` at `now_ms`.
    ///
    /// * Returns [`Ok(())`] if enough tokens are available (they are
    ///   atomically debited).
    /// * Returns [`Err(retry_after_ms)`] when the bucket cannot cover the
    ///   request; the returned value is the minimum number of milliseconds to
    ///   wait before the request should succeed.
    ///
    /// Requesting more tokens than the [`capacity`](RateLimiter::capacity) can
    /// never succeed and always yields a (possibly very large) wait time.
    pub fn try_acquire(&mut self, now_ms: u64, tokens: u32) -> Result<(), u64> {
        self.refill(now_ms);
        let cost = (tokens as u64).saturating_mul(MICRO_PER_TOKEN);
        if self.acc_micro >= cost {
            self.acc_micro -= cost;
            Ok(())
        } else {
            let deficit = cost - self.acc_micro;
            if self.micro_per_ms == 0 {
                // No refill rate configured: the bucket will never refill.
                return Err(u64::MAX);
            }
            // Ceil division implemented safely (no overflow on the numerator).
            let q = deficit / self.micro_per_ms;
            let retry_after_ms = if deficit.is_multiple_of(self.micro_per_ms) {
                q
            } else {
                q.saturating_add(1)
            };
            Err(retry_after_ms)
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(10, 10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C6: acquire/debit works and an empty bucket reports a correct wait time.
    #[test]
    fn rate_limiter_acquire_and_retry_after() {
        let mut rl = RateLimiter::new(10, 10);
        assert_eq!(rl.available_tokens(), 10);

        assert_eq!(rl.try_acquire(0, 5), Ok(()));
        assert_eq!(rl.available_tokens(), 5);
        assert_eq!(rl.try_acquire(0, 5), Ok(()));
        assert_eq!(rl.available_tokens(), 0);

        // Empty: 1 token at 10 tokens/s needs 100 ms.
        assert_eq!(rl.try_acquire(0, 1), Err(100));

        // After 100ms exactly one token has refilled.
        assert_eq!(rl.try_acquire(100, 1), Ok(()));
        assert_eq!(rl.available_tokens(), 0);

        // Two more tokens need 200ms more.
        assert_eq!(rl.try_acquire(100, 2), Err(200));
    }

    /// C7: extreme / inverted clocks and huge requests stay bounded and panic-free.
    #[test]
    fn rate_limiter_u64_max_capacity_bound() {
        let mut rl = RateLimiter::new(1_000, 100);
        let cap = rl.capacity();

        // A single request far in the future must fill the bucket to exactly the
        // capacity and never overflow.
        assert_eq!(rl.try_acquire(u64::MAX, 1), Ok(()));
        assert!(rl.available_tokens() <= cap);

        // The bucket is bounded by capacity regardless of elapsed time.
        let mut big = RateLimiter::new(1, 5);
        big.try_acquire(u64::MAX, 1).unwrap();
        assert!(big.available_tokens() <= big.capacity());

        // Inverted clock (now < last_refill) must saturate, not panic.
        let _ = big.try_acquire(0, 1); // now rewound
        assert!(big.available_tokens() <= big.capacity());

        // A request larger than capacity must always be rejected.
        let huge = big.try_acquire(0, big.capacity() + 1000).unwrap_err();
        assert!(huge > 0);
    }
}
