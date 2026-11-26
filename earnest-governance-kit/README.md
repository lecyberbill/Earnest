# Earnest Governance & BCPE Kit

Bring Earnest's **Contract-First Invariant Governance** (Mode 3 Architecture -> SHA-256 lock -> Mode 2 Constrained `shadow_worktree` + `project-integrity-gate`) and the **BCPE Experience Knowledge Base** (`earnest-bcpe`) to **any MCP-compatible AI IDE or CLI**:

- **Claude Code**
- **Cursor**
- **Windsurf / Cline / Roo Code**
- **OpenAI Codex CLI / Antigravity**

---

## 1. Install (One-Time Setup)

### Windows (PowerShell)

```powershell
.\earnest-governance-kit\install.ps1
```

This builds `earnest_gov_mcp.exe` and `kb.exe` in `--release` mode and copies them (along with `kb.db` if present) into `%LOCALAPPDATA%\Earnest\bin`.

Add `%LOCALAPPDATA%\Earnest\bin` to your user `PATH` (if not already present), then open a new terminal.

### macOS / Linux

```bash
cargo build --release --manifest-path earnest-ide/core/Cargo.toml --bin earnest_gov_mcp
cargo build --release --manifest-path earnest-bcpe/Cargo.toml
mkdir -p ~/.local/bin
cp earnest-ide/core/target/release/earnest_gov_mcp ~/.local/bin/
cp earnest-bcpe/target/release/kb ~/.local/bin/
```

---

## 2. Connect Your IDE

### Claude Code

In your target project root:

```bash
cp path/to/Hernest/earnest-governance-kit/mcp.example.json .mcp.json
cp path/to/Hernest/earnest-governance-kit/AGENTS.md CLAUDE.md
```

Or register globally via CLI:

```bash
claude mcp add earnest-governance -- earnest_gov_mcp --workspace .
claude mcp add earnest-bcpe -- kb mcp
```

### Cursor

Copy `mcp.example.json` to `.cursor/mcp.json` (or global `~/.cursor/mcp.json`) and copy `AGENTS.md` to `.cursorrules` (or `AGENTS.md`) at the root of your project.

### Windsurf / Other MCP Clients

Add the two servers from `mcp.example.json` (`earnest-governance` and `earnest-bcpe`) to your client's MCP configuration file, and include `AGENTS.md` in your project rules.

---

## 3. Available MCP Tools

### `earnest-governance` (`earnest_gov_mcp`)

| Tool | Purpose |
|------|---------|
| `gov_status` | Inspect current mode (`normal` / `constrained`), SHA-256 lock validity, contract count, and staged shadow files |
| `gov_add_invariant` | Add or update a verifiable contract (`cargo_test` or `static`) in `.ide/invariants.json` |
| `gov_seal_architecture` | Compute SHA-256 lock (`.ide/invariants.lock`, read-only) and switch to `constrained` mode |
| `gov_write_code` | Stage a file in `.ide/shadow_worktree` (`stage_only: true`) or validate & merge immediately |
| `gov_shadow_check` | Run `cargo test` / `cargo clippy` / `pytest` / `node --test` inside `.ide/shadow_worktree` |
| `gov_shadow_validate_and_merge` | Run all sealed probes + `project-integrity-gate` and merge to the real workspace on success |

### `earnest-bcpe` (`kb mcp`)

- `kb_list_products`, `kb_search`, `kb_get`, `kb_check`: query known pitfalls and solutions before coding.
- `kb_propose`, `kb_report_incident`, `kb_confirm`: record newly discovered bugs and fixes.

---

## 4. Typical Workflow

1. Ask your agent: *"Check `kb_list_products` / `kb_search`, define contracts with `gov_add_invariant`, and seal them with `gov_seal_architecture`."*
2. Ask it to implement in `.ide/shadow_worktree` using `gov_write_code(..., stage_only: true)` and `gov_shadow_check`.
3. Finish with `gov_shadow_validate_and_merge`: **the real workspace is never modified unless every probe and compiler/linter check passes.**

---

## 5. Troubleshooting

- **`earnest_gov_mcp` or `kb` not found**: verify `%LOCALAPPDATA%\Earnest\bin` (Windows) or `~/.local/bin` (Unix) is in your `PATH`, or use absolute paths in `.mcp.json`.
- **Custom `kb.db` location**: set `EARNEST_KB_DB=/path/to/kb.db` in the `env` block of `earnest-bcpe` inside `.mcp.json`.
