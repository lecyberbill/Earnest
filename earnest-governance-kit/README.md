# Earnest Governance & BCPE Kit (for Claude Code, Cursor, Codex & MCP IDEs)

Use Earnest's **invariant-driven development** (Mode 3 Architecture -> SHA-256 lock -> Mode 2 Constrained `shadow_worktree` + `project-integrity-gate`) and the **BCPE experience knowledge base** (`earnest-bcpe`) from any MCP-compatible IDE or CLI.

## What is included

- `earnest_gov_mcp` (`earnest-ide/core/src/bin/earnest_gov_mcp.rs`): MCP stdio server exposing:
  - `gov_status`, `gov_add_invariant`, `gov_seal_architecture`
  - `gov_write_code`, `gov_shadow_check`, `gov_shadow_validate_and_merge`
- `kb` (`earnest-bcpe`): MCP stdio server + SQLite knowledge base (`kb.db`) exposing `kb_list_products`, `kb_search`, `kb_propose`, `kb_report_incident`, etc.
- `mcp.example.json`: ready-to-use `.mcp.json` snippet for Claude Code / Cursor / Windsurf.
- `AGENTS.md`: drop-in agent instructions (also usable as `CLAUDE.md`).
- `install.ps1`: builds both release binaries on Windows and copies them with `kb.db` into `%LOCALAPPDATA%\Earnest\bin`.

## Quick Setup (Windows)

```powershell
# 1. Build and install binaries + kb.db
.\earnest-governance-kit\install.ps1

# 2. Copy mcp.example.json as .mcp.json in your target project
Copy-Item .\earnest-governance-kit\mcp.example.json C:\path\to\your-project\.mcp.json

# 3. Copy AGENTS.md (or CLAUDE.md) in your target project
Copy-Item .\earnest-governance-kit\AGENTS.md C:\path\to\your-project\CLAUDE.md
```

## Quick Setup (macOS / Linux)

```bash
cargo build --release --manifest-path earnest-ide/core/Cargo.toml --bin earnest_gov_mcp
cargo build --release --manifest-path earnest-bcpe/Cargo.toml
```

Then point `.mcp.json` to `earnest_gov_mcp` and `kb` (see `mcp.example.json`).
