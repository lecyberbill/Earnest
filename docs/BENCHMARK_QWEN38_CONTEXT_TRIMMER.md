# Benchmark & Field Report: Qwen 3.8 27B (Local Ollama / Radeon) on `context-trimmer`

**Date:** 2026-11-15  
**Model:** `qwen3.8:27b` via local Ollama (`aurora-dev`, AMD Radeon GPU)  
**Target Project:** `context-trimmer` (`D:\Projet\context-trimmer`, zero-dependency Rust library)  

---

## 1. Objective

Evaluate **Qwen 3.8 27B** running locally on a Radeon GPU inside **Earnest IDE** under full **Mode 3 (Spec-First) → Mode 2 (Constrained)** governance, building a production-ready crate (`context-trimmer`) for Earnest's own ReAct loop.

---

## 2. Results

### Phase A — Mode 3 (Spec-First)
- **Duration:** `134,262 ms` (~2m14s), `22,321` tokens.
- **Outcome:** Designed a clean, zero-dependency architecture (`Role`, `ToolCall`, `Message`, `truncate_tool_output`, `trim_history`) and submitted 4 sealed contracts (`C1..C4`) with exact `cargo test -- --exact` function names.
- **Notable detail:** Proactively accounted for Unicode character boundaries (`chars().count()`) so multi-byte UTF-8 characters are never split.

### Phase B — Mode 2 (Constrained)
- **Self-correction:** After an initial multi-file diff attempt failed, Qwen 3.8 27B diagnosed the single-file `write_code` constraint on its own and delivered `Cargo.toml`, `src/lib.rs`, `src/trimmer.rs`, `tests/trimmer_invariants.rs`, and `.ide/atlas.md` cleanly.
- **Verification:** **100% of `C1..C4` `cargo_test` probes passed** with real assertions covering:
  - `C1`: Head/tail truncation around `TRUNCATION_MARKER` + UTF-8 combining accents (`e\u{301}`) + unchanged output under limit.
  - `C2`: Unconditional pinning of the first `System` message even when `budget == 0`.
  - `C3`: Strict character budget enforcement on a contiguous recent window.
  - `C4`: Atomic `ToolCall` / `ToolResult` unit grouping (`units_of`) so no orphaned tool results can ever be produced.

---

## 3. Integration

- `context-trimmer` (`v0.1.0`) was integrated directly into `earnest-ide/core/Cargo.toml` (commit `629e079`) and verified with `cargo test` (`5/5` passing).
