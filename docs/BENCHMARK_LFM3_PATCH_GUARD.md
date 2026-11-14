# Benchmark & Field Report: LFM 3 on `patch-guard`

**Date:** 2026-11-14  
**Model:** LFM 3 (Local via LM Studio)  
**Target Project:** `patch-guard` (`D:\Projet\Test_local`, zero-dependency Rust library)  

---

## 1. Objective

Evaluate how a small, fast local model (LFM 3) performs inside **Earnest IDE** on a 4-file Rust library (`path_guard.rs`, `patch_check.rs`, `lib.rs`, `tests/guard_tests.rs`) across:
- **Mode 3 (Archi Spec-First)** & **Mode 2 (Constrained)**
- **Mode 1 (Normal)**

---

## 2. Observations

### Phase A — Mode 3 (Spec-First) & Mode 2 (Constrained)
- **Schema friction:** On `submit_invariants`, LFM 3 initially struggled with nested JSON (`"description"` instead of `"statement"`, `"probs"` instead of `"probes"`, literal `"cargo_test|cli|static"`).
- **Engineering fix shipped:** We added a flat form-style tool `add_invariant(id, title, statement, target_file, test_query)` in `earnest-ide/core/src/engine/react.rs` (commit `91a3d96`) plus auto-healing in `submit_invariants` (commit `24b4425`). With `add_invariant`, LFM 3 passed Mode 3 on the first try.
- **Probe design bottleneck:** In Mode 2, LFM 3 still wrote regexes (`^/.*$`, `\x00`) into `cargo_test` queries and free-form sentences into `static` queries, causing probes to fail repeatedly until hitting `max_steps`.

### Phase B — Mode 1 (Normal)
- **Scaffolding:** LFM 3 created all requested files (`Cargo.toml`, `src/path_guard.rs`, `src/patch_check.rs`, `src/lib.rs`, `tests/guard_tests.rs`, `DISCOVERIES.md`, `.ide/atlas.md`) and compiled cleanly.
- **Limitations observed:**
  - Left placeholder tests (`0 passed; 0 failed` in `cargo test`).
  - Ran `cargo test` in the default CWD (`earnest_ide_desktop_lib`) without noticing it wasn't testing `D:\Projet\Test_local`.
  - Continued calling `kb_search` / `kb_propose` / `invoke_subagent` after finishing the files.

---

## 3. Takeaways

| Capability | LFM 3 (Local 3B) | Stronger Models (Qwen 3.8 27B / DeepSeek) |
| :--- | :--- | :--- |
| **Mode 1 Scaffolding** | Fast & functional | Fast & complete |
| **Mode 3 (`add_invariant`)** | Works with flat form tool | Works reliably |
| **Mode 2 (Probe-driven)** | Not recommended (loops on probe failures) | Recommended |
| **Real Test Assertions** | Tends to leave placeholders | Writes full assertions |
