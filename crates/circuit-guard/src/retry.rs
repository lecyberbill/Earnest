//! A deterministic **Retry / Backoff policy** with bounded exponential growth,
//! reproducible jitter and `Retry-After` header support.
//!
//! # Determinism
//! The jitter is produced by a self-contained [SplitMix64](RetryPolicy)
//! generator with an **injectable seed**. The same seed always yields the same
//! sequence of delays, which makes backoff schedules fully reproducible in
//! tests and in production replays.
//!
//! # Boundedness
//! Every computed delay is capped at `max_ms` using saturating arithmetic, so
//! `attempt = 100`, `attempt = u64::MAX`, or a `Retry-After` of `u64::MAX`
//! seconds can never overflow or panic.
//!
//! ```
//! use circuit_guard::RetryPolicy;
//!
//! let mut policy = RetryPolicy::new(100, 5_000, 42);
//! // Pure bounded exponential value (no jitter).
//! assert_eq!(policy.backoff_ms(0), 100);
//! assert!(policy.backoff_ms(100) <= 5_000);
//!
//! // Jittered, reproducible, and still bounded.
//! assert!(policy.jittered_backoff_ms(3) <= 5_000);
//!
//! // A 2-second Retry-After header is honored and capped at max_ms.
//! assert_eq!(policy.backoff_with_retry_after(3, Some(2)), 2_000);
//! assert_eq!(policy.backoff_with_retry_after(3, Some(10)), 5_000);
//! ```

/// A small, dependency-free, seedable SplitMix64 pseudo-random generator.
///
/// It is intentionally minimal and `const`-friendly: only pure integer
/// arithmetic with wrapping is used, so the sequence is fully deterministic
/// for a given seed.
#[derive(Debug, Clone)]
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Creates a generator from an initial `seed`.
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Advances the state and returns the next 64-bit value.
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Returns a value in the half-open range `[0, bound)`.
    ///
    /// A `bound` of `0` yields `0`. The modulo introduces a mild bias that is
    /// acceptable for jitter and keeps the routine panic-free and pure.
    fn next_below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            0
        } else {
            self.next_u64() % bound
        }
    }
}

/// A deterministic retry/backoff policy.
///
/// Construct with [`RetryPolicy::new`]. The jitter state is advanced by the
/// mutating methods, so call sites that need reproducibility should be driven
/// from a single `RetryPolicy` instance in a fixed order.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    base_ms: u64,
    max_ms: u64,
    rng: SplitMix64,
}

impl RetryPolicy {
    /// Creates a backoff policy.
    ///
    /// * `base_ms` — the base delay in milliseconds (clamped to a minimum of
    ///   `1`). The pure exponential value for attempt `n` is
    ///   `base_ms * 2^n`.
    /// * `max_ms` — hard upper bound applied to every computed delay (clamped
    ///   to a minimum of `1`).
    /// * `seed` — seed for the deterministic jitter generator.
    pub fn new(base_ms: u64, max_ms: u64, seed: u64) -> Self {
        Self {
            base_ms: base_ms.max(1),
            max_ms: max_ms.max(1),
            rng: SplitMix64::new(seed),
        }
    }

    /// Returns the configured base delay in milliseconds.
    pub fn base_ms(&self) -> u64 {
        self.base_ms
    }

    /// Returns the configured maximum delay in milliseconds.
    pub fn max_ms(&self) -> u64 {
        self.max_ms
    }

    /// Returns the current internal PRNG state (useful for snapshotting and
    /// replaying a deterministic jitter sequence).
    pub fn rng_state(&self) -> u64 {
        self.rng.state
    }

    /// Returns the **pure** bounded exponential delay for `attempt`:
    /// `min(base_ms * 2^attempt, max_ms)`.
    ///
    /// This value is jitter-free and fully `const`-pure with respect to the
    /// policy. It is saturating: `attempt = 100` or `u64::MAX` never panics.
    pub fn backoff_ms(&self, attempt: u64) -> u64 {
        // If base alone already meets/exceeds the cap, short-circuit.
        if self.base_ms >= self.max_ms {
            return self.max_ms;
        }
        // For attempt >= 64, 2^attempt overflows u64, so base * 2^attempt
        // certainly exceeds max_ms (since base >= 1).
        if attempt >= 64 {
            return self.max_ms;
        }
        // Safe: attempt < 64, so `1u64 << attempt` cannot overflow.
        let factor = 1u64 << attempt;
        match self.base_ms.checked_mul(factor) {
            Some(v) => v.min(self.max_ms),
            None => self.max_ms,
        }
    }

    /// Returns a **jittered** bounded delay for `attempt` ("equal jitter").
    ///
    /// The result lies in the range `[base/2, base]` where `base` is the pure
    /// [`backoff_ms`](RetryPolicy::backoff_ms) value, and is always capped at
    /// `max_ms`. Advances the deterministic PRNG, so it is `&mut self`.
    pub fn jittered_backoff_ms(&mut self, attempt: u64) -> u64 {
        let base = self.backoff_ms(attempt);
        if base == 0 {
            return 0;
        }
        let half = base / 2;
        // rand in [0, base) -> rand/2 in [0, base/2) -> total in [base/2, base).
        let rand = self.rng.next_below(base);
        half.saturating_add(rand / 2).min(self.max_ms)
    }

    /// Computes the delay for `attempt` honoring an optional `Retry-After`
    /// header expressed in whole **seconds**.
    ///
    /// * `Some(secs)` → `min(secs * 1000, max_ms)` (saturating, so
    ///   `secs = u64::MAX` is safe).
    /// * `None` → falls back to [`jittered_backoff_ms`](RetryPolicy::jittered_backoff_ms).
    pub fn backoff_with_retry_after(&mut self, attempt: u64, retry_after_secs: Option<u64>) -> u64 {
        match retry_after_secs {
            Some(secs) => secs.saturating_mul(1000).min(self.max_ms),
            None => self.jittered_backoff_ms(attempt),
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new(100, 30_000, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C8: backoff is bounded by `max_ms` and the jitter sequence is deterministic
    /// (same seed ⇒ same sequence) and reproducible, even at extreme attempts.
    #[test]
    fn retry_policy_bounded_deterministic() {
        let max = 5_000u64;

        // Pure exponential is exact and bounded.
        let p = RetryPolicy::new(100, max, 42);
        assert_eq!(p.backoff_ms(0), 100);
        assert_eq!(p.backoff_ms(1), 200);
        assert_eq!(p.backoff_ms(2), 400);
        // 100 * 2^7 = 12800 capped to max.
        assert_eq!(p.backoff_ms(7), max);
        // Extreme attempts never panic and stay bounded.
        assert_eq!(p.backoff_ms(100), max);
        assert_eq!(p.backoff_ms(u64::MAX), max);

        // Jittered values are bounded and within [base/2, base].
        let mut a = RetryPolicy::new(100, max, 42);
        let mut b = RetryPolicy::new(100, max, 42);
        let seq_a: Vec<u64> = (0..12).map(|i| a.jittered_backoff_ms(i)).collect();
        let seq_b: Vec<u64> = (0..12).map(|i| b.jittered_backoff_ms(i)).collect();
        assert_eq!(seq_a, seq_b, "same seed must yield the same jitter sequence");
        for (i, v) in seq_a.iter().enumerate() {
            let base = RetryPolicy::new(100, max, 0).backoff_ms(i as u64);
            assert!(*v <= max);
            if base != 0 {
                assert!(*v >= base / 2, "equal-jitter lower bound violated");
                assert!(*v <= base, "jitter must not exceed the base value");
            }
        }

        // Different seeds should (with overwhelming probability) differ.
        let mut c = RetryPolicy::new(100, max, 43);
        let seq_c: Vec<u64> = (0..12).map(|i| c.jittered_backoff_ms(i)).collect();
        assert_ne!(seq_a, seq_c);
    }

    /// C9: a `Retry-After` header (seconds) is honored and capped at `max_ms`.
    #[test]
    fn retry_policy_respects_retry_after() {
        let max = 5_000u64;
        let mut p = RetryPolicy::new(100, max, 42);

        // 2 seconds -> 2000ms (under the cap).
        assert_eq!(p.backoff_with_retry_after(3, Some(2)), 2_000);
        // 10 seconds -> 10000ms capped to max.
        assert_eq!(p.backoff_with_retry_after(3, Some(10)), max);
        // 0 seconds -> 0ms.
        assert_eq!(p.backoff_with_retry_after(3, Some(0)), 0);
        // u64::MAX seconds must saturate, not overflow.
        assert_eq!(p.backoff_with_retry_after(3, Some(u64::MAX)), max);
        // None falls back to the jittered value, bounded.
        let v = p.backoff_with_retry_after(3, None);
        assert!(v <= max);
    }
}
