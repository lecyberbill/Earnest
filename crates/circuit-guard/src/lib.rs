//! # circuit_guard
//!
//! A deterministic resilience toolkit for LLM and HTTP calls.
//!
//! `circuit_guard` provides three composable, side-effect-free primitives
//! that can be driven by an *injectable* millisecond clock. This makes every
//! component fully deterministic and unit-testable: there is **no**
//! `std::thread::sleep`, **no** `unsafe` code and **no** external dependency
//! in the crate.
//!
//! | Primitive            | Purpose                                                    |
//! |----------------------|------------------------------------------------------------|
//! | [`CircuitBreaker`]   | Fail fast after a run of consecutive failures, then probe.  |
//! | [`RateLimiter`]      | Token-bucket rate limiting with a precise `retry_after_ms`. |
//! | [`RetryPolicy`]      | Bounded exponential backoff with deterministic jitter.      |
//!
//! ## Clock injection
//!
//! Every state-changing / state-querying method takes an explicit
//! `now_ms: u64` argument. Pass `std::time::SystemTime::now()` converted to
//! milliseconds in production, or a hand-rolled counter in tests. All
//! arithmetic is saturating, so feeding `u64::MAX` (or a backwards clock)
//! never panics or overflows.
//!
//! ## Quick start
//!
//! ```
//! use circuit_guard::{CircuitBreaker, RateLimiter, RetryPolicy};
//!
//! // Circuit breaker: open after 3 consecutive failures.
//! let mut breaker = CircuitBreaker::new(3, 1_000, 2);
//! let mut t: u64 = 0;
//! for _ in 0..2 {
//!     assert!(breaker.allow_request(t).is_ok());
//!     breaker.record_failure(t);
//! }
//! // Third failure trips the breaker.
//! assert!(breaker.allow_request(t).is_ok());
//! breaker.record_failure(t);
//! assert!(breaker.allow_request(t).is_err());
//!
//! // Rate limiter: 10 tokens capacity, refill 5 per second.
//! let mut limiter = RateLimiter::new(5, 10);
//! assert!(limiter.try_acquire(0, 10).is_ok());
//! assert_eq!(limiter.try_acquire(0, 1), Err(200)); // needs 1 more token at 5/s
//!
//! // Retry policy: deterministic backoff, bounded and jittered.
//! let policy = RetryPolicy::new(100, 5_000, 42);
//! assert_eq!(policy.backoff_ms(0), 100);
//! assert!(policy.backoff_ms(100) <= 5_000);
//! ```

//! All public items are re-exported at the crate root for ergonomic use.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod breaker;
mod limiter;
mod retry;

pub use breaker::{CircuitBreaker, CircuitError, Permit, BreakerState};
pub use limiter::RateLimiter;
pub use retry::RetryPolicy;
