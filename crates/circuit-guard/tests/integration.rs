//! Integration test: a simulated LLM/HTTP client combining the three
//! `circuit_guard` primitives against an injectable millisecond clock.
//!
//! The whole run is deterministic: the same scripted timeline always produces
//! the same decisions, with no sleeping thread anywhere.

use circuit_guard::{
    CircuitBreaker, CircuitError, BreakerState, Permit, RateLimiter, RetryPolicy,
};

/// A scripted, deterministic stand-in for a flaky HTTP/LLM endpoint.
struct FakeEndpoint {
    /// Scripted per-call outcomes, consumed in order.
    outcomes: Vec<bool>,
    /// `Retry-After` header value (whole seconds) served when set.
    retry_after_secs: Option<u64>,
    /// Total number of calls actually accepted by the endpoint.
    accepted: usize,
}

impl FakeEndpoint {
    fn new(outcomes: Vec<bool>) -> Self {
        Self {
            outcomes,
            retry_after_secs: None,
            accepted: 0,
        }
    }

    /// Simulates one HTTP/LLM call. Returns `Ok(())` on success, or
    /// `Err(retry_after_secs)` on failure.
    fn call(&mut self) -> Result<(), Option<u64>> {
        self.accepted += 1;
        match self.outcomes.first().copied() {
            Some(true) => {
                self.outcomes.remove(0);
                Ok(())
            }
            Some(false) => {
                self.outcomes.remove(0);
                Err(self.retry_after_secs)
            }
            // Out of scripted outcomes: behave as success.
            None => Ok(()),
        }
    }
}

/// Drives the resilience stack for one logical request, honoring the breaker,
/// the rate limiter and the retry policy. Returns `true` if the underlying
/// call eventually succeeded within the attempt budget.
fn call_with_resilience(
    breaker: &mut CircuitBreaker,
    limiter: &mut RateLimiter,
    policy: &mut RetryPolicy,
    endpoint: &mut FakeEndpoint,
    now_ms: u64,
    max_attempts: u64,
) -> Result<(), (CircuitError, u64)> {
    for attempt in 0..max_attempts {
        // 1. Circuit breaker gate.
        // 1. Circuit breaker gate (the permit is a marker; the breaker has
        //    already admitted the call when this is `Ok`).
        if let Err(err) = breaker.allow_request(now_ms) {
            return Err((err, now_ms));
        }

        // 2. Rate limiter gate (fast path; if empty we stop with a wait hint).
        if let Err(retry_after_ms) = limiter.try_acquire(now_ms, 1) {
            // Surface the limiter wait through the retry policy for uniformity.
            let _ = policy; // policy not consulted on a hard limiter rejection
            return Err((
                CircuitError::Open {
                    retry_after_ms,
                },
                now_ms + retry_after_ms,
            ));
        }

        // 3. Issue the real call.
        match endpoint.call() {
            Ok(()) => {
                breaker.record_success(now_ms);
                return Ok(());
            }
            Err(retry_after_secs) => {
                breaker.record_failure(now_ms);
                // 4. Compute the backoff (honoring Retry-After if present).
                let delay =
                    policy.backoff_with_retry_after(attempt, retry_after_secs);
                // Advance the deterministic clock by the computed delay.
                let _next = now_ms.saturating_add(delay);
                // Loop: the breaker will now fast-reject or retry on next attempt.
            }
        }
    }
    Err((CircuitError::HalfOpenSaturated, now_ms))
}

/// The full scripted scenario from the crate README.
#[test]
fn full_resilience_scenario_is_deterministic() {
    let mut breaker = CircuitBreaker::new(3, 1_000, 2);
    let mut limiter = RateLimiter::new(100, 10); // 100/s, burst 10
    let mut policy = RetryPolicy::new(100, 5_000, 7);
    // Flaky: fails the first 3 calls (opens breaker), then succeeds.
    let mut endpoint = FakeEndpoint::new(vec![false, false, false, true]);

    // First request: 3 failures trip the breaker into Open.
    let res = call_with_resilience(&mut breaker, &mut limiter, &mut policy, &mut endpoint, 0, 5);
    assert!(res.is_err(), "breaker must reject after threshold");
    assert_eq!(breaker.state(), BreakerState::Open);

    // Fast-reject while open (before the 1s cooldown).
    let early = breaker.allow_request(500);
    assert!(matches!(early, Err(CircuitError::Open { retry_after_ms: 500 })));

    // After cooldown, a successful probe closes the circuit.
    let permit = breaker.allow_request(1_000).expect("half-open probe admitted");
    let _ = permit;
    breaker.record_success(1_001);
    assert_eq!(breaker.state(), BreakerState::Closed);
    assert_eq!(breaker.allow_request(1_002), Ok(Permit));
}

/// The rate limiter alone enforces the sustained rate with exact wait hints.
#[test]
fn limiter_enforces_rate_with_exact_wait() {
    let mut limiter = RateLimiter::new(10, 5); // 10/s, burst 5
    // Burst out 5 tokens at t=0.
    for _ in 0..5 {
        assert_eq!(limiter.try_acquire(0, 1), Ok(()));
    }
    // Now empty: one token needs 100 ms at 10/s.
    assert_eq!(limiter.try_acquire(0, 1), Err(100));
    // Exactly at t=100 the single token is available again.
    assert_eq!(limiter.try_acquire(100, 1), Ok(()));
    // ... and is immediately exhausted again.
    assert_eq!(limiter.try_acquire(100, 1), Err(100));
}

/// The retry policy produces a bounded, reproducible schedule across attempts.
#[test]
fn retry_schedule_is_bounded_and_reproducible() {
    let mut a = RetryPolicy::new(100, 1_000, 1234);
    let mut b = RetryPolicy::new(100, 1_000, 1234);
    for attempt in 0..20u64 {
        let da = a.jittered_backoff_ms(attempt);
        let db = b.jittered_backoff_ms(attempt);
        assert_eq!(da, db, "same seed must give same delay at attempt {attempt}");
        assert!(da <= 1_000, "delay {da} exceeds the 1s cap");
    }
    // Extreme attempt stays bounded.
    assert!(a.jittered_backoff_ms(100) <= 1_000);
}
