# Benchmark Run — `agent-scheduler` (DeepSeek + Earnest IDE Governance)

**Date**: 2026-11-20  
**Target Crate**: `crates/agent-scheduler` (zero-dependency Rust library, `#![forbid(unsafe_code)]`, `std` only)  
**Workflow**: Mode 3 (Spec-First) → Cryptographic Lock (SHA-256) → Mode 2 (Constrained Shadow Worktree)

---

## 1. Architecture & Contracts Sealed (Mode 3)

- **Modules**: `src/lib.rs`, `src/task.rs`, `src/graph.rs`, `src/transitions.rs`, `src/scheduler.rs`, `src/summary.rs`
- **Key Design Choices**:
  - `BTreeMap`/`BTreeSet` + monotonic `seq` (`priority DESC, seq ASC`) for 100% deterministic scheduling.
  - Pure `ready_set()` / `next_ready(max_concurrency)` vs transactional `start_batch(max_concurrency)`.
  - Atomic `add_tasks()` batch with forward-reference validation; `cancel(id, propagate)` transitive cascade; `fail` stays local (`blocked_count` in `ProgressSummary`).
- **Contracts**: 16 (`C1`–`C16`), including `C16-P1` static AST check (`no_external_dependencies = true`).

---

## 2. Verification Results

- `cargo test --manifest-path crates\agent-scheduler\Cargo.toml --all-targets`: **17/17 passed** (`graph`, `scheduler`, `transitions`, `summary`, `task` invariants)
- `cargo clippy --manifest-path crates\agent-scheduler\Cargo.toml --all-targets -- -D warnings`: **0 warnings, 0 errors**
- `.ide/atlas.md` generated and merged alongside source files.
