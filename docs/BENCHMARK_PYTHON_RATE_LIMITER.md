# Benchmark Run — Python Token Bucket Rate Limiter (LFM vs Qwen Local + Earnest IDE Governance)

**Date**: 2026-11-19  
**Target Project**: `D:\Projet\test_py` (Python stdlib `sqlite3`, `dataclasses`, `time`, `typing` + `pytest`)  
**Workflow**: Mode 3 (Spec-First) → Cryptographic Lock (SHA-256) → Mode 2 (Constrained Shadow Worktree)

---

## 1. A/B Observation (LFM vs Qwen Local)

- **LFM**:
  - Struggled with Mode 3/Mode 2 tool-calling discipline (repeated `list_files`/`read_file`, dumped raw code in prose until Mode 3 anti-stall was added, tripped on non-blocking `kb_search` `unknown_product` response and tried calling `kb_propose` instead of `write_code`).
- **Qwen (local inference)**:
  - Designed a clean modular architecture (`src/rate_limiter.py`, `tests/test_rate_limiter.py`) with `Store` (`Protocol`), `MemoryStore`, `SQLiteStore` (`PRAGMA journal_mode=WAL`, `BEGIN IMMEDIATE ... COMMIT/ROLLBACK`), injectable `clock`, lazy refill, and strict `ValueError` validation.
  - Formalized and sealed **8 contracts (`C1`–`C8`)**:
    - `C1` (High): Initial burst at full capacity
    - `C2` (High): Rejection + exact `retry_after` (`inf` when `refill_rate == 0`)
    - `C3` (High): Deterministic refill via injectable `clock` (no `time.sleep`)
    - `C4` (High): Atomic SQLite WAL persistence + `MemoryStore` parity
    - `C5` (High): Strict isolation per `client_id`
    - `C6` (High): Strict parameter validation (`ValueError`)
    - `C7` (Medium): `reset()` restores capacity (no-op for unknown client)
    - `C8` (Medium): `state()` reflects elapsed refill at read time

---

## 2. Verification Results

- `python -m pytest D:\Projet\test_py -v`: **8/8 passed** in `0.20s` (`test_initial_burst`, `test_reject_retry_after`, `test_refill_injectable_clock`, `test_sqlite_atomic_persistence`, `test_client_isolation`, `test_invalid_params`, `test_reset_client`, `test_state_query`).

---

## 3. Earnest IDE Hardening Triggered by This Run

- Exposed the flat `add_invariant` tool schema in `earnest-ide/core/src/tools/mod.rs` and added Mode 3 anti-stall nudges in `earnest-ide/core/src/engine/react.rs`.
- Added automatic `git worktree prune` before `git worktree add` in `earnest-ide/core/src/governance/shadow.rs` to recover cleanly when `.ide/shadow_worktree` is missing on disk.
