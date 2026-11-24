---
name: earnest-wfl
description: Design, validate, seal, and execute deterministic WFL v1.0 workflows via the `wfl` MCP server (`wf-mcp.exe`) in Earnest.
---

# Earnest WFL (`earnest-wf-engine`)

Deterministic YAML workflow language for orchestrating files, loops, conditions, scripts, HTTP APIs, and structured data.

## Available MCP Tools (`wfl`)

- `mcp__wfl__wfl_get_grammar`: returns the WFL JSON schema and registered workflows.
- `mcp__wfl__wfl_validate`: validates strict YAML syntax and semantics (DAG, connections, `sha256`).
- `mcp__wfl__wfl_save_draft`: saves a valid workflow as `draft` in `.wf/` and SQLite WAL.
- `mcp__wfl__wfl_run`: executes a workflow (`dry_run: true` while `draft`; `dry_run: false` allowed after sealing).
- `mcp__wfl__wfl_seal`: seals a `draft` workflow with its SHA-256 hash to allow real execution.
- `mcp__wfl__wfl_inspect_run` / `mcp__wfl__wfl_list_workflows`: run inspection and governance.

## Mandatory Agent Protocol

1. Draft the WFL YAML and check it with `wfl_validate`.
2. Save it with `wfl_save_draft`.
3. Simulate with `wfl_run` (`dry_run: true`), then seal with `wfl_seal` before `dry_run: false`.
4. Never edit a `.wf.yaml` file on disk after sealing without running `wfl_save_draft` + `wfl_seal` again.

## Variables, Expressions & Pipes (`${...}` or `{{...}}`)

- Equivalent syntaxes: `${vars.name}` or `{{ vars.name }}` (also `inputs.<name>`, `outputs.<name>`, `env.<VAR>`, `keyring.<secret>`).
- **Global `vars:`**: evaluated at startup (e.g., `today: "${now('%Y-%m-%d')}"`, `out: "${vars.base}/export"`).
- **Increment / Arithmetic** in `flow.set`: `value: "${vars.counter} + 1"` (`+`, `-`, `*`, `/`).
- **Built-in pipes**:
  - Paths / strings: `| stem` (filename without extension), `| ext`, `| basename`, `| dirname`, `| upper`, `| lower`, `| trim`, `| replace('a','b')`, `| split(',')`
  - Arrays: `| length`, `| first`, `| last`, `| join('\n')`, `| map('stem')`, `| sort`, `| unique`
  - Dates: `${now('%Y-%m-%d')}` or `${now('%Y%m%d_%H%M%S')}`

## Available Actions

### VFS / Files
- `vfs.list` (`conn`, `path`, `glob`, `recursive`, `include_dirs`) -> returns `{ data, names, stems, paths, full_paths, count }`. Each item in `data` has `{ name, stem, ext, path, full_path, dir, is_dir, size }`.
- `vfs.read` (`conn`, `path`, `encoding: utf8|base64`) -> `{ content }`.
- `vfs.write` (`conn`, `path`, `content`, `create_dirs`)
- `vfs.append` (`conn`, `path`, `content`, `newline: true`, `create_dirs: true`)
- `vfs.mkdir` (`conn`, `path`)
- `vfs.exists` (`conn`, `path`) -> `{ exists }`
- `vfs.transfer` / `vfs.move` (`src: {conn, path}`, `dst: {conn, path}`, `overwrite`)
- `vfs.stat` (`conn`, `path`, `compute_sha256`) -> `{ size, is_file, sha256 }`
- `vfs.delete` (`conn`, `path`)

### Control Flow
- `flow.if` (`condition`, `then: [...]`, `else: [...]`) — operators: `==`, `!=`, `>`, `<`, `>=`, `<=`, `&&`, `||`, `!`, `contains`, `starts_with`, `ends_with`, `matches`, `in`.
- `flow.each` (`items`, `as: item`, `steps: [...]`) — exposes `${loop.index}`, `${loop.index1}`, `${loop.count}`, `${loop.first}`, `${loop.last}`.
- `flow.while` (`condition`, `max_iterations: 1000`, `steps: [...]`)
- `flow.switch` (`value`, `cases: { val: [...] }`, `default: [...]`)
- `flow.break` / `flow.continue`
- `flow.set` / `flow.append` (`params: { var, value }`)
- `flow.call` (`params: { workflow_path, inputs }`)
- `flow.sleep` (`params: { duration_ms }`) / `flow.fail` (`params: { code, message }`)

### Data & System
- `data.parse_json` (`params: { text }`)
- `data.parse_csv` (`params: { text, delimiter: ",", headers: true }`) -> `{ rows, count }`
- `data.regex` (`params: { text, pattern }`) -> `{ matches, count, first }`
- `sys.exec` (`params: { program, args, cwd, env, allow_non_zero }`, `timeout_ms`) -> `{ exit_code, stdout, stderr }`
- `sys.http` (`params: { conn, method, url, headers, query, body, parse_json }`, `timeout_ms`) -> `{ status, body }`
- `sys.log` (`params: { level: info|warn|error|debug, message }`)
