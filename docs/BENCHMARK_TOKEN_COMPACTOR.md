# Benchmark Run — `token-compactor` (Qwen Local + Earnest IDE Governance)

**Date**: 2026-11-20  
**Target Crate**: `crates/token-compactor` (zero-dependency Rust library, `std` only)  
**Workflow**: Mode 3 (Spec-First) → Cryptographic Lock (SHA-256) → Mode 2 (Constrained Shadow Worktree)

---

## 1. Architecture & Contracts Sealed (Mode 3)

- **Modules**: `src/lib.rs`, `src/estimator.rs`, `src/compactor.rs`
- **Contracts**: 11 (`C1`–`C11`)
  - `C1`–`C2`: Deterministic `TokenEstimator` (`MESSAGE_OVERHEAD = 4`, `CHARS_PER_TOKEN = 4`)
  - `C3`–`C4`: Protected initial `System` message, last `User` prompt, and `keep_recent` window
  - `C5`–`C7`: 3-phase compaction (summarize old `Tool` outputs → bounded truncation → evict oldest intermediate turns)
  - `C8`–`C9`: Explicit `ZeroBudget` and `BudgetBelowMinimum` (`MIN_BUDGET = 2 * MESSAGE_OVERHEAD`) errors
  - `C10`: Consistent `CompactionStats` (`original_tokens`, `compacted_tokens`, `messages_removed`, `messages_truncated`, `tool_responses_summarized`)
  - `C11`: Strict budget invariant (`estimate_messages(result) <= budget` always holds)

---

## 2. Verification Results

- `cargo test --manifest-path crates\token-compactor\Cargo.toml --all-targets`: **10/10 passed**
- `cargo clippy --manifest-path crates\token-compactor\Cargo.toml --all-targets -- -D warnings`: **0 warnings, 0 errors**
- `shadow_validate_and_merge`: `merged_to_workspace: true`, zero external dependencies.
