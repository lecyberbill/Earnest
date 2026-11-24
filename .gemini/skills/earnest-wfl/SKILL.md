---
name: earnest-wfl
description: Concevoir, valider, sceller et exécuter des workflows déterministes WFL v1.0 via le serveur MCP `wfl` (`wf-mcp.exe`) dans Earnest.
---

# Earnest WFL (`earnest-wf-engine`)

Langage de workflow YAML déterministe pour orchestrer fichiers, boucles, conditions, scripts, API HTTP et données.

## Outils MCP disponibles (`wfl`)

- `mcp__wfl__wfl_get_grammar` : schéma JSON WFL + liste des workflows.
- `mcp__wfl__wfl_validate` : validation YAML stricte + sémantique (DAG, connexions, `sha256`).
- `mcp__wfl__wfl_save_draft` : enregistre en statut `draft` dans `.wf/` et SQLite WAL.
- `mcp__wfl__wfl_run` : exécute (`dry_run: true` tant que `draft`, `dry_run: false` après scellement).
- `mcp__wfl__wfl_seal` : scelle via SHA-256 pour autoriser l'exécution réelle.
- `mcp__wfl__wfl_inspect_run` / `mcp__wfl__wfl_list_workflows` : suivi et gouvernance.

## Protocole obligatoire pour les agents

1. Rédiger le YAML WFL et le vérifier avec `wfl_validate`.
2. Sauvegarder avec `wfl_save_draft`.
3. Simuler avec `wfl_run` (`dry_run: true`), puis sceller avec `wfl_seal` avant `dry_run: false`.
4. Ne jamais modifier le `.wf.yaml` sur disque après scellement sans repasser par `wfl_save_draft` + `wfl_seal`.

## Variables, Expressions & Pipes (`${...}` ou `{{...}}`)

- Syntaxes équivalentes : `${vars.nom}` ou `{{ vars.nom }}` (aussi `inputs.<nom>`, `outputs.<nom>`, `env.<VAR>`, `keyring.<secret>`).
- **Variables globales `vars:`** : évaluées au démarrage (ex. `date_jour: "${now('%Y-%m-%d')}"`, `out: "${vars.base}/export"`).
- **Incrément / Arithmétique** dans `flow.set` : `value: "${vars.compteur} + 1"` (`+`, `-`, `*`, `/`).
- **Pipes intégrés** :
  - Chemins/texte : `| stem` (nom sans extension), `| ext`, `| basename`, `| dirname`, `| upper`, `| lower`, `| trim`, `| replace('a','b')`, `| split(',')`
  - Tableaux : `| length`, `| first`, `| last`, `| join('\n')`, `| map('stem')`, `| sort`, `| unique`
  - Date : `${now('%Y-%m-%d')}` ou `${now('%Y%m%d_%H%M%S')}`

## Actions disponibles

### VFS / Fichiers
- `vfs.list` (`conn`, `path`, `glob`, `recursive`, `include_dirs`) → retourne `{ data, names, stems, paths, full_paths, count }`. Chaque élément de `data` contient `{ name, stem, ext, path, full_path, dir, is_dir, size }`.
- `vfs.read` (`conn`, `path`, `encoding: utf8|base64`) → `{ content }`.
- `vfs.write` (`conn`, `path`, `content`, `create_dirs`)
- `vfs.append` (`conn`, `path`, `content`, `newline: true`, `create_dirs: true`)
- `vfs.mkdir` (`conn`, `path`)
- `vfs.exists` (`conn`, `path`) → `{ exists }`
- `vfs.transfer` / `vfs.move` (`src: {conn, path}`, `dst: {conn, path}`, `overwrite`)
- `vfs.stat` (`conn`, `path`, `compute_sha256`) → `{ size, is_file, sha256 }`
- `vfs.delete` (`conn`, `path`)

### Contrôle de flux
- `flow.if` (`condition`, `then: [...]`, `else: [...]`) — opérateurs : `==`, `!=`, `>`, `<`, `>=`, `<=`, `&&`, `||`, `!`, `contains`, `starts_with`, `ends_with`, `matches`, `in`.
- `flow.each` (`items`, `as: item`, `steps: [...]`) — expose `${loop.index}`, `${loop.index1}`, `${loop.count}`, `${loop.first}`, `${loop.last}`.
- `flow.while` (`condition`, `max_iterations: 1000`, `steps: [...]`)
- `flow.switch` (`value`, `cases: { val: [...] }`, `default: [...]`)
- `flow.break` / `flow.continue`
- `flow.set` / `flow.append` (`params: { var, value }`)
- `flow.call` (`params: { workflow_path, inputs }`)
- `flow.sleep` (`params: { duration_ms }`) / `flow.fail` (`params: { code, message }`)

### Données & Système
- `data.parse_json` (`params: { text }`)
- `data.parse_csv` (`params: { text, delimiter: ",", headers: true }`) → `{ rows, count }`
- `data.regex` (`params: { text, pattern }`) → `{ matches, count, first }`
- `sys.exec` (`params: { program, args, cwd, env, allow_non_zero }`, `timeout_ms`) → `{ exit_code, stdout, stderr }`
- `sys.http` (`params: { conn, method, url, headers, query, body, parse_json }`, `timeout_ms`) → `{ status, body }`
- `sys.log` (`params: { level: info|warn|error|debug, message }`)
