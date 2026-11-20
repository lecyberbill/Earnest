# Benchmark Run — `tool-guard` (Qwen Local + Earnest IDE Sandbox Tools)

**Date**: 2026-11-20  
**Target Crate**: `crates/tool-guard` (zero-dependency Rust library, `std` only)  
**Workflow**: Mode 3 (Spec-First) → Cryptographic Lock (SHA-256) → Mode 2 (Constrained Shadow Worktree)

---

## 1. Architecture & Contracts Sealed (Mode 3)

- **Timing**: Mode 3 completed in ~3 minutes (`11:56:04` → `11:59:23`) after adding the Mode 3 anti-code-bloat rule (`e346c7b`).
- **Modules**: `src/lib.rs`, `src/error.rs`, `src/path.rs`, `src/file.rs`, `src/params.rs`
- **Contracts**: 10 (`C1`–`C10`)
  - `C1`: Path traversal (`..`) blocked before system-path classification
  - `C2`: Sensitive system paths rejected (`/etc`, `C:\Windows`, `/proc`, `~/.ssh`)
  - `C3`: Empty paths rejected
  - `C4`: Normalized paths (cross-platform `/` output)
  - `C5`: Binary file detection (NUL byte / non-printable ratio)
  - `C6`: File size bound (`10 MiB` default)
  - `C7`: Path alias resolution (`resolve_path_alias`)
  - `C8`: Numeric bounds (`clamp_numeric`, `clamp_f64`, rejecting `NaN`/`Infinity`)
  - `C9`: Actionable `GuardError` / `ToolGuardError` `Display` messages for LLMs
  - `C10`: Zero external dependencies (`Cargo.toml` static AST check)

---

## 2. Verification Results

- `cargo test --manifest-path crates\tool-guard\Cargo.toml --all-targets`: **9/9 passed** (`test_path_traversal_blocked`, `test_system_paths_rejected`, `test_empty_path_rejected`, `test_path_normalization`, `test_binary_file_detection`, `test_file_size_bound`, `test_path_alias_resolution`, `test_numeric_bounds`, `test_error_display_actionable`)
- `cargo clippy --manifest-path crates\tool-guard\Cargo.toml --all-targets -- -D warnings`: **0 warnings, 0 errors**
- SHA-256 seal (`da93aceb…`) preserved intact across Mode 2 (`shadow_check` + `shadow_validate_and_merge`).
