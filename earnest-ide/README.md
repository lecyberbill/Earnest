# Earnest IDE Pro

Cockpit de développement agentique **Zéro-Build** (Rust natif + Tauri v2 + Monaco Editor) avec gouvernance par invariants cryptographiques et isolation par Shadow Worktree.

## Architecture

- **`earnest-ide/core/`** — Backend Rust (`earnest-ide-core`, port `8090` ou `--mcp` stdio) :
  - `src/engine/react.rs` : Boucle ReAct copilote avec exécution parallèle read-only, anti-loop et compaction de contexte.
  - `src/governance/` : Mode 3 (Spec-First & Invariants SHA-256) → Mode 2 (Shadow Worktree + sondes AST/CLI) → Mode 1 (Libre).
  - `src/lsp/` : Client **LSP générique JSON-RPC 2.0** (`client.rs`) avec framing `Content-Length`, synchronisation `didOpen`/`didChange`, complétion, survol, `goto definition` et diagnostics pour 12 langages (`rust`, `python`, `typescript`, `javascript`, `go`, `c`, `cpp`, `toml`, `yaml`, `json`, `html`, `css`), avec repli sur `jedi` / `rust.rs` / `indexer.rs`.
  - `src/server/` : Serveur HTTP REST + SSE (`/api/ide/lsp/catalog`, `/api/ide/lsp/status`, `/api/ide/lsp/install`, `/api/ide/settings`, `/api/ide/governance/*`, `/api/ide/pty/*`).
- **`earnest-ide/ui/`** — Interface Vanilla ES Modules sans bundler :
  - `js/app.js` : Orchestrateur principal (layout, onglets, Git, diagnostics, terminaux, ReAct, projets).
  - `js/components/kb-view.js` : Vue Base d'Expérience (`BCPE`).
  - `js/features/monaco-lsp.js` : Providers Monaco (`completion`, `hover`, `definition`, `signatureHelp`).
  - `js/features/lsp-settings.js` : Catalogue LSP dans les paramètres et bannière d'installation automatique à l'ouverture d'un fichier non couvert.
  - `js/features/file-watcher.js` : Détection des modifications externes sur les onglets ouverts.
- **`earnest-ide/src-tauri/`** — Wrapper desktop natif Tauri v2.

## Raccourcis utiles

- `Ctrl+P` : Palette de fichiers (Quick Open)
- `Ctrl+Shift+F` : Recherche globale Regex
- `Ctrl+K` : Édition IA inline dans Monaco
- `F12` / `Ctrl+Click` : Aller à la définition (LSP)
