# circuit_guard

A **deterministic resilience toolkit** for LLM and HTTP calls, written in pure
Rust (`std` only, **zero** external dependencies, `#![forbid(unsafe_code)]`).

`circuit_guard` composes three side-effect-free primitives that are driven by an
**injectable millisecond clock**. Because no component ever blocks, sleeps, or
reads the wall clock, the whole stack is trivially deterministic and unit-testable.

| Primitive      | Type        | Purpose                                                    |
|----------------|-------------|------------------------------------------------------------|
| `CircuitBreaker` | struct   | Open / Half-Open / Closed state machine that fast-rejects a failing dependency. |
| `RateLimiter`    | struct   | Token-bucket rate limiter with drift-free, continuous refill. |
| `RetryPolicy`    | struct   | Bounded exponential backoff with deterministic jitter and `Retry-After` support. |

## Why deterministic?

Every public API takes the current time as a `now_ms: u64` argument. This means:

- **No `std::thread::sleep`** anywhere in the core.
- **No global state**, no hidden clock.
- **Reproducible tests and production replays**: the same scripted timeline always
  yields the same decisions.

## Features

- ✅ Deterministic, injectable millisecond clock (`u64`)
- ✅ Zero dependencies, `std` only, `#![forbid(unsafe_code)]`
- ✅ `#![deny(missing_docs)]` — every public item is documented
- ✅ Saturating arithmetic — **no panic** on `u64::MAX` clocks, inverted clocks, or
  `attempt = 100`
- ✅ Drift-free token bucket (lazy refill with a fractional micro-token remainder)
- ✅ Deterministic jitter via a seedable SplitMix64 PRNG

## Quick start

```rust
use circuit_guard::{CircuitBreaker, RateLimiter, RetryPolicy, CircuitError};

// 3. Circuit breaker: open after 3 consecutive failures, 1s cooldown, 2 probes.
let mut breaker = CircuitBreaker::new(3, 1_000, 2);

// 2. Rate limiter: 100 requests/s sustained, burst of 10.
let mut limiter = RateLimiter::new(100, 10);

// 1. Retry policy: base 100ms, cap 5s, seed 7 (reproducible jitter).
let mut policy = RetryPolicy::new(100, 5_000, 7);

let mut now_ms: u64 = 0;

for attempt in 0..5u64 {
    // Gate 1: circuit breaker.
    if let Err(err) = breaker.allow_request(now_ms) {
        eprintln!("blocked: {err}");
        break;
    }
    // Gate 2: rate limiter.
    if let Err(retry_after_ms) = limiter.try_acquire(now_ms, 1) {
        eprintln!("throttled, retry in {retry_after_ms} ms");
        now_ms = now_ms.saturating_add(retry_after_ms);
        continue;
    }
    // Gate 3: call the (flaky) dependency, then record the outcome.
    match call_llm() {
        Ok(resp) => { breaker.record_success(now_ms); return Ok(resp); }
        Err(e) => {
            breaker.record_failure(now_ms);
            // Honor `Retry-After` when the dependency provided one.
            let retry_after_secs = e.retry_after_secs();
            let delay = policy.backoff_with_retry_after(attempt, retry_after_secs);
            now_ms = now_ms.saturating_add(delay);
        }
    }
}
Err(())
```

## API reference

### `CircuitBreaker`

```text
                 failure_threshold failures
   +--------+  ---------------------------->  +------
   | Closed |                                 | Open |
   +--------+ <-----------------------------  +------+
            success                       open_duration_ms elapsed
                 (half-open probe)        (limited probes allowed)
```

- `CircuitBreaker::new(failure_threshold, open_duration_ms, half_open_max_probes)`
- `allow_request(&mut self, now_ms) -> Result<Permit, CircuitError>`
- `record_success(&mut self, now_ms)`
- `record_failure(&mut self, now_ms)`
- `state(&self) -> BreakerState` · `consecutive_failures(&self) -> u32`

`CircuitError`:
- `Open { retry_after_ms }` — circuit open; the hint is how long to wait.
- `HalfOpenSaturated` — half-open but already admitting its probe budget.

### `RateLimiter`

- `RateLimiter::new(rate_per_sec, capacity)`
- `try_acquire(&mut self, now_ms, tokens) -> Result<(), u64>`
  - `Ok(())` — tokens granted (debited).
  - `Err(retry_after_ms)` — bucket cannot cover the request; wait that many ms.
- `available_tokens(&self) -> u32` · `capacity(&self) -> u32` · `rate_per_sec(&self) -> u64`

Refill is **continuous and drift-free**: the fractional remainder is carried in
integer micro-token units, so irregular call cadences never lose tokens.

### `RetryPolicy`

- `RetryPolicy::new(base_ms, max_ms, seed)`
- `backoff_ms(&self, attempt) -> u64` — **pure** bounded exponential:
  `min(base_ms * 2^attempt, max_ms)`.
- `jittered_backoff_ms(&mut self, attempt) -> u64` — **equal jitter**, bounded,
  reproducible for a given seed.
- `backoff_with_retry_after(&mut self, attempt, Option<secs>) -> u64` — honors a
  `Retry-After` header (seconds), capped at `max_ms`; falls back to jitter when
  `None`.

## Guarantees

- **No blocking**: no `sleep`, no `unsafe`.
- **No overflow / no panic**: all arithmetic is saturating or `checked`; extreme
  inputs (`u64::MAX`, `attempt = 100`, inverted clocks) are handled gracefully.
- **Bounded**: every delay is capped at the configured `max_ms` / `capacity`.

## Testing

```bash
cargo test                                   # unit + integration + doctests
cargo clippy --all-targets -- -D warnings    # lints, deny warnings
```

## License

MIT
