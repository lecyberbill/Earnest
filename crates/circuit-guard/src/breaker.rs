//! A deterministic **Circuit Breaker** driven by an injectable millisecond clock.
//!
//! The breaker is a pure state machine: it never blocks, never sleeps and never
//! touches the wall clock. All time is provided by the caller as `now_ms: u64`,
//! which makes the whole component trivially deterministic and testable.
//!
//! State machine:
//!
//! ```text
//!                 failure_threshold failures
//!   +--------+  ---------------------------->  +------+
//!   | Closed |                                 | Open |
//!   +--------+ <-----------------------------  +------+
//!            success                       open_duration_ms elapsed
//!                 (half-open probe)        (limited probes allowed)
//! ```
//!
//! * `Closed` — requests flow. Consecutive failures are counted.
//! * `Open` — requests are fast-rejected until `open_duration_ms` elapses.
//! * `HalfOpen` — at most `half_open_max_probes` probe requests are admitted;
//!   the first success closes the circuit, any failure re-opens it.

use std::fmt;

/// The observable state of a [`CircuitBreaker`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BreakerState {
    /// The circuit is closed and requests are allowed to flow.
    Closed,
    /// The circuit is open and requests are fast-rejected.
    Open,
    /// The circuit is half-open and a limited number of probe requests is admitted.
    HalfOpen,
}

/// A permit authorizing a caller to issue exactly one downstream request.
///
/// The permit is stateless; it merely records that the breaker admitted the
/// call. Record the outcome with [`CircuitBreaker::record_success`] or
/// [`CircuitBreaker::record_failure`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Permit;

/// Errors returned by [`CircuitBreaker::allow_request`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitError {
    /// The circuit is open. `retry_after_ms` is the (approximate) number of
    /// milliseconds that must elapse before the circuit may become half-open.
    Open {
        /// Suggested wait time in milliseconds before retrying.
        retry_after_ms: u64,
    },
    /// The circuit is half-open and is already admitting its maximum number of
    /// probe requests.
    HalfOpenSaturated,
}

impl fmt::Display for CircuitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CircuitError::Open { retry_after_ms } => write!(
                f,
                "circuit is open, retry after {} ms",
                retry_after_ms
            ),
            CircuitError::HalfOpenSaturated => write!(
                f,
                "circuit is half-open and already allowing the maximum number of probes"
            ),
        }
    }
}

impl std::error::Error for CircuitError {}

/// A deterministic circuit breaker state machine.
///
/// Construct with [`CircuitBreaker::new`], then drive it using the injectable
/// clock APIs. No thread is ever blocked.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    state: BreakerState,
    consecutive_failures: u32,
    failure_threshold: u32,
    open_duration_ms: u64,
    half_open_max_probes: u32,
    probes_in_flight: u32,
    /// `now_ms` at which the breaker last transitioned to (or refreshed) `Open`.
    opened_at: u64,
}

impl CircuitBreaker {
    /// Creates a breaker.
    ///
    /// * `failure_threshold` — consecutive failures (in `Closed`) that open the
    ///   circuit. Clamped to a minimum of `1`.
    /// * `open_duration_ms` — cooldown while the circuit stays open before it
    ///   becomes half-open.
    /// * `half_open_max_probes` — maximum concurrent probe requests admitted
    ///   while half-open. Clamped to a minimum of `1`.
    pub fn new(failure_threshold: u32, open_duration_ms: u64, half_open_max_probes: u32) -> Self {
        Self {
            state: BreakerState::Closed,
            consecutive_failures: 0,
            failure_threshold: failure_threshold.max(1),
            open_duration_ms,
            half_open_max_probes: half_open_max_probes.max(1),
            probes_in_flight: 0,
            opened_at: 0,
        }
    }

    /// Returns the current [`BreakerState`].
    pub fn state(&self) -> BreakerState {
        self.state
    }

    /// Returns the number of consecutive failures currently recorded.
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    /// Decides, at `now_ms`, whether a request may proceed.
    ///
    /// * In `Closed`, always returns [`Ok(Permit)`].
    /// * In `Open`, returns [`CircuitError::Open`] with a `retry_after_ms`
    ///   hint, or transitions to `HalfOpen` (admitting a probe) once the
    ///   cooldown has elapsed.
    /// * In `HalfOpen`, admits a probe while fewer than `half_open_max_probes`
    ///   are in flight, otherwise [`CircuitError::HalfOpenSaturated`].
    pub fn allow_request(&mut self, now_ms: u64) -> Result<Permit, CircuitError> {
        match self.state {
            BreakerState::Closed => Ok(Permit),
            BreakerState::Open => {
                let elapsed = now_ms.saturating_sub(self.opened_at);
                if elapsed >= self.open_duration_ms {
                    self.state = BreakerState::HalfOpen;
                    self.probes_in_flight = 1;
                    Ok(Permit)
                } else {
                    let retry_after_ms = self.open_duration_ms - elapsed;
                    Err(CircuitError::Open { retry_after_ms })
                }
            }
            BreakerState::HalfOpen => {
                if self.probes_in_flight < self.half_open_max_probes {
                    self.probes_in_flight = self
                        .probes_in_flight
                        .saturating_add(1)
                        .min(self.half_open_max_probes);
                    Ok(Permit)
                } else {
                    Err(CircuitError::HalfOpenSaturated)
                }
            }
        }
    }

    /// Records a successful downstream call at `now_ms`.
    ///
    /// A success while `HalfOpen` closes the circuit and resets the failure
    /// counter.
    pub fn record_success(&mut self, now_ms: u64) {
        match self.state {
            BreakerState::HalfOpen => {
                self.state = BreakerState::Closed;
                self.consecutive_failures = 0;
                self.probes_in_flight = 0;
            }
            BreakerState::Open => {
                // Defensive: a success should not occur while open (no permit
                // is ever issued there), but we recover gracefully by closing.
                self.state = BreakerState::Closed;
                self.consecutive_failures = 0;
                self.probes_in_flight = 0;
            }
            BreakerState::Closed => {
                self.consecutive_failures = 0;
            }
        }
        // `now_ms` is intentionally retained for clock monotonicity guarantees.
        let _ = now_ms;
    }

    /// Records a failed downstream call at `now_ms`.
    ///
    /// * In `Closed`, the failure is counted; reaching the threshold opens the
    ///   circuit.
    /// * In `HalfOpen`, any failure immediately re-opens the circuit and resets
    ///   the cooldown.
    /// * In `Open`, the cooldown is refreshed (fail-fast is preserved).
    pub fn record_failure(&mut self, now_ms: u64) {
        match self.state {
            BreakerState::Closed => {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                if self.consecutive_failures >= self.failure_threshold {
                    self.state = BreakerState::Open;
                    self.opened_at = now_ms;
                }
            }
            BreakerState::HalfOpen => {
                self.state = BreakerState::Open;
                self.opened_at = now_ms;
                self.probes_in_flight = 0;
            }
            BreakerState::Open => {
                self.opened_at = now_ms;
            }
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(3, 60_000, 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C1: the breaker opens once the failure threshold of consecutive failures is met.
    #[test]
    fn breaker_opens_after_threshold() {
        let mut cb = CircuitBreaker::new(3, 100, 1);
        assert_eq!(cb.state(), BreakerState::Closed);

        // Two failures: still closed.
        cb.record_failure(0);
        assert_eq!(cb.state(), BreakerState::Closed);
        cb.record_failure(10);
        assert_eq!(cb.state(), BreakerState::Closed);
        assert!(cb.allow_request(20).is_ok());

        // Third failure: opens the circuit.
        cb.record_failure(30);
        assert_eq!(cb.state(), BreakerState::Open);
        assert!(matches!(cb.allow_request(31), Err(CircuitError::Open { .. })));

        // A success before the threshold must reset the counter.
        let mut fresh = CircuitBreaker::new(3, 100, 1);
        fresh.record_failure(0);
        fresh.record_success(5);
        assert_eq!(fresh.consecutive_failures(), 0);
        fresh.record_failure(6);
        fresh.record_failure(7);
        assert_eq!(fresh.state(), BreakerState::Closed);
    }

    /// C2: while half-open, only `half_open_max_probes` requests are admitted.
    #[test]
    fn breaker_half_open_probe_limit() {
        let mut cb = CircuitBreaker::new(1, 100, 2);
        cb.record_failure(0); // open (threshold = 1)
        assert_eq!(cb.state(), BreakerState::Open);

        // Before cooldown elapses: rejected.
        assert!(matches!(cb.allow_request(50), Err(CircuitError::Open { .. })));

        // After cooldown: half-open, admits exactly `half_open_max_probes`.
        assert_eq!(cb.allow_request(100), Ok(Permit)); // probe 1
        assert_eq!(cb.allow_request(101), Ok(Permit)); // probe 2
        assert_eq!(cb.allow_request(102), Err(CircuitError::HalfOpenSaturated));
    }

    /// C3: a successful half-open probe closes the circuit and resets failures.
    #[test]
    fn breaker_half_open_success_closes() {
        let mut cb = CircuitBreaker::new(1, 100, 3);
        cb.record_failure(0);
        assert_eq!(cb.state(), BreakerState::Open);

        let permit = cb.allow_request(100).expect("half-open probe admitted");
        let _ = permit;
        cb.record_success(101);
        assert_eq!(cb.state(), BreakerState::Closed);
        assert_eq!(cb.consecutive_failures(), 0);
        assert_eq!(cb.allow_request(102), Ok(Permit));
    }

    /// C4: a failed half-open probe re-opens the circuit and resets the cooldown.
    #[test]
    fn breaker_half_open_failure_reopens() {
        let mut cb = CircuitBreaker::new(1, 100, 3);
        cb.record_failure(0); // open at t=0
        assert_eq!(cb.state(), BreakerState::Open);

        // Probe admitted after first cooldown.
        assert_eq!(cb.allow_request(100), Ok(Permit));
        cb.record_failure(100); // fails -> re-opened, cooldown resets to t=100
        assert_eq!(cb.state(), BreakerState::Open);

        // Still open 50ms later (cooldown restarted at t=100).
        assert!(matches!(cb.allow_request(150), Err(CircuitError::Open { .. })));
        // Half-open again after the new cooldown.
        assert_eq!(cb.allow_request(200), Ok(Permit));
    }

    /// C5: extreme and inverted clocks never panic (saturating arithmetic).
    #[test]
    fn breaker_u64_max_no_panic() {
        let max = u64::MAX;

        // Open at t=0, query at u64::MAX -> half-open, never panics.
        let mut cb = CircuitBreaker::new(1, 100, 1);
        cb.record_failure(0);
        assert_eq!(cb.state(), BreakerState::Open);
        assert_eq!(cb.allow_request(max), Ok(Permit));

        // Success at u64::MAX closes it.
        cb.record_success(max);
        assert_eq!(cb.state(), BreakerState::Closed);

        // Inverted clock (now < opened_at) must saturate to 0, not panic.
        let mut cb2 = CircuitBreaker::new(1, 100, 1);
        cb2.record_failure(max); // open at u64::MAX
        assert_eq!(cb2.state(), BreakerState::Open);
        assert!(matches!(cb2.allow_request(0), Err(CircuitError::Open { .. })));

        // record_failure while already open refreshes without panic.
        cb2.record_failure(5);
        assert_eq!(cb2.state(), BreakerState::Open);
    }
}