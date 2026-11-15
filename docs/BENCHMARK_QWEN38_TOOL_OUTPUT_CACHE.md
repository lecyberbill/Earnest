# Benchmark & Field Report: Qwen 3.8 27B (Local Ollama / Radeon) on `tool-output-cache`

**Date:** 2026-11-15  
**Model:** `qwen3.8:27b` via local Ollama (`aurora-dev`, AMD Radeon GPU)  
**Target Project:** `tool-output-cache` (`D:\Projet\tool-output-cache`, zero-dependency Rust library)  

---

## 1. Objective

Build a zero-dependency LRU `ToolCache` for Earnest IDE's ReAct loop under **Mode 3 (Spec-First) → Mode 2 (Constrained)** governance, caching read-only tool outputs (`read_file`, `list_files`, `grep_search`) and invalidating entries when files change.

---

## 2. Results

- **Mode 3 (Spec-First):** Designed a clean `HashMap` + LRU recency architecture with `max_entries == 0` safe degeneration and targeted `invalidate_path` (exact `path_hint` + directory prefix matching for `list_files`/`grep_search`).
- **Mode 2 (Constrained):** Delivered `Cargo.toml`, `src/lib.rs`, `src/cache.rs`, and `tests/cache_tests.rs`.
- **Verification (`cargo test`):** **14/14 tests passed (0 failed)** in `0.84s`:
  - `2` unit tests (`normalize_handles_separators_and_root`, `overlap_detects_ancestry_both_ways`)
  - `11` integration tests (`cache_hit`, `cache_miss`, LRU eviction, path invalidation, `clear()`, `stats()`, zero-dependency check)
  - `1` doc-test

---

## 3. Integration

- `tool-output-cache` (`v0.1.0`) was integrated into `earnest-ide/core/Cargo.toml` alongside `kv-wal-engine` and `context-trimmer`.
