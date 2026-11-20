# Benchmark Run — `loop-detector` (Qwen Local + Earnest IDE Sandbox Tools)

**Date**: 2026-11-20  
**Target Crate**: `crates/loop-detector` (zero-dependency Rust library, `std` only)  
**Workflow**: Mode 3 (Spec-First) → Cryptographic Lock (SHA-256) → Mode 2 (Constrained Shadow Worktree)

---

## 1. Architecture & Contracts Sealed (Mode 3)

- **Modules**: `src/lib.rs`, `src/config.rs`, `src/verdict.rs`, `src/detector.rs`, `src/rules/{mod,exact_repeat,stagnation,alternation}.rs`
- **Contracts**: 10 (`C1`–`C10`)
  - `C1`: Strict config validation (`ConfigError` on invalid window/thresholds)
  - `C2`: Exact repetition detection (`ExactRepeatRule`)
  - `C3`: Stagnation detection (`StagnationRule`)
  - `C4`: Alternation cycle detection (`AlternationRule`, `A-B-A-B` / `A-B-C`)
  - `C5`: Graduated `Verdict` / `Severity` (`NoLoop < Warning < LoopDetected`)
  - `C6`: `reset_progress()` clears sliding window
  - `C7`: Bounded FIFO `VecDeque` window (`len <= window_size`)
  - `C8`: Deterministic evaluation (`evaluate()`)
  - `C9`: Zero external dependencies (`Cargo.toml` check)
  - `C10`: No panic on empty/extreme inputs

---

## 2. A/B Impact of Mode 2 Sandbox Enhancements

- **Before (`write_code` all-or-nothing + Windows `&&` bug)**:
  - Probe runner failed on Windows PowerShell 5.1 due to `&&` chaining between `cargo test` and `cargo clippy`; multi-file staging drifted across retries and hit output token limits on resume.
- **After (`shadow_check`, `shadow_copy`, `shadow_validate_and_merge`, PowerShell `; if ($LASTEXITCODE -ne 0)`)**:
  - ~4x faster run completion with zero failed handoffs: staged 9 files (`stage_only: true`), iterated via `shadow_check` to fix borrow/threshold/clippy issues without burning probe failure budget, then promoted via `shadow_validate_and_merge` at zero token cost.

---

## 3. Verification Results

- `cargo test --manifest-path crates\loop-detector\Cargo.toml --all-targets`: **31/31 passed**
- `cargo clippy --manifest-path crates\loop-detector\Cargo.toml --all-targets -- -D warnings`: **0 warnings, 0 errors**

---

## 4. Integration into Earnest

- Added `loop-detector` dependency to `earnest-core/Cargo.toml` (`13/13` tests passing) and `earnest-ide/core/Cargo.toml` (`12/12` tests passing).
- Wired `LoopDetector` into both `ReActEngine` loops (`earnest-core/src/engine/react.rs` and `earnest-ide/core/src/engine/react.rs`) to detect exact repeats, stagnation, and alternation cycles (`A-B-A-B`), with `reset_progress()` on write/progress tools.
