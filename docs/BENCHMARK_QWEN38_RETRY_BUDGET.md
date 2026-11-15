# Benchmark Run — `retry-budget` (Qwen 3.8 Local + Earnest IDE Governance)

**Date**: 2026-11-15  
**Target Crate**: `D:\Projet\retry-budget` (zero-dependency Rust library)  
**Model**: Qwen 3.8 (local server)  
**Workflow**: Mode 3 (Spec-First) → Cryptographic Lock → Mode 2 (Constrained Shadow Worktree)

---

## 1. Architecture & Contracts Sealed (Mode 3)

- **Contracts**: 5 (`C1`–`C5`)
  - `C1` (Critical): Valid constructor (`new` never panics; `window_size == 0` ⇒ always denied)
  - `C2` (Critical): Sliding-window FIFO eviction
  - `C3` (Critical): Circuit breaker (`failures >= max_failures` ⇒ `is_retry_allowed() == false`)
  - `C4` (High): `reset()` clears window and restores `is_retry_allowed() == true`
  - `C5` (High): `stats()` consistency & `Send + Sync`
- **SHA-256 Lock**: `53faf98fb6a4446cc105fbdf54d713e180f277b14403728e04d3c4e39b026566`

---

## 2. Verification Results

- **Unit tests (`src/lib.rs`)**: 20/20 passed
- **Integration tests (`tests/integration.rs`)**: 7/7 passed (after fixing a 1-line math assertion in `scenario_failure_storm_opens_then_auto_recovers`: `failures == 2` instead of `0` after 3 failures + 4 successes in a window of 6)
- **Doc-tests**: 1/1 passed

---

## 3. Integration into Earnest IDE

- Added `retry-budget = { path = "../../../retry-budget" }` to `earnest-ide/core/Cargo.toml`.
- Wired `retry_budget::RetryBudget::new(10, 5)` into `ReActEngine::run_with_callback` (`earnest-ide/core/src/engine/react.rs`) to guard LLM completion calls with a sliding-window circuit breaker.
