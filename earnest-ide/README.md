# Earnest IDE Pro

**Zero-Build** agentic development cockpit (native Rust + Tauri v2 + Monaco Editor) with cryptographic invariant governance and Shadow Worktree isolation.

## Architecture

- **`earnest-ide/core/`** — Rust backend (`earnest-ide-core`, port `8090` or `--mcp` stdio):
  - `src/engine/react.rs`: ReAct copilot loop with parallel read-only tool execution, loop detection, and context compaction.
  - `src/governance/`: Mode 3 (Spec-First & SHA-256 Invariants) → Mode 2 (Shadow Worktree + AST/CLI probes) → Mode 1 (Unrestricted).
  - `src/lsp/`: Generic **JSON-RPC 2.0 LSP** client (`client.rs`) with `Content-Length` framing, `didOpen`/`didChange` sync, completion, hover, `goto definition`, and live diagnostics (`publishDiagnostics` → Monaco `setModelMarkers`) across 12 languages (`rust`, `python`, `typescript`, `javascript`, `go`, `c`, `cpp`, `toml`, `yaml`, `json`, `html`, `css`), with `jedi` / `rust.rs` / `indexer.rs` fallback.
  - `src/llm/`: Streaming LLM client wired to `crates/stream-cleaner` (`<thought>` / `<think>` separation).
  - `src/server/`: HTTP REST + SSE server (`/api/ide/lsp/*`, `/api/ide/kb/products|entries|create|annotate|delete`, `/api/ide/settings`, `/api/ide/governance/*`, `/api/ide/pty/*`).
- **`earnest-ide/ui/`** — Bundler-free Vanilla ES Modules UI:
  - `js/app.js`: Main orchestrator (layout, tabs, Git, diagnostics, terminals, ReAct, projects, `renderSafeMarkdown`).
  - `js/components/kb-view.js`: Experience Knowledge Base (`BCPE`) view with manual product creation, product filter, and entry deletion.
  - `js/features/monaco-lsp.js`: Monaco providers (`completion`, `hover`, `definition`, `signatureHelp`, `diagnostics` markers).
  - `js/features/lsp-settings.js`: LSP server catalog in Settings and automatic missing-LSP install banner on file open.
  - `js/features/file-watcher.js`: External file-change detection on open tabs.
- **`earnest-ide/src-tauri/`** — Lightweight native Tauri v2 desktop wrapper.

## Useful Shortcuts

- `Ctrl+P`: Quick Open file palette
- `Ctrl+Shift+F`: Global Regex Search
- `Ctrl+K`: Inline AI Edit in Monaco
- `F12` / `Ctrl+Click`: Go to Definition (LSP)
