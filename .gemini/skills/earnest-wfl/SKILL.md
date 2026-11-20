---
name: earnest-wfl
description: Concevoir, valider, sceller et exécuter des workflows déterministes WFL v0.1 via le serveur MCP `wfl` (`wf-mcp.exe`) dans Earnest.
---

# Earnest WFL (`earnest-wf-engine`)

Utilise ce workflow déterministe dès qu'un agent doit orchestrer une chaîne d'étapes reproductible (`vfs_read`, `vfs_write`, `vfs_list`, `http_get`, `if`, `each`, `on_error`) plutôt que d'enchaîner des appels ad hoc.

## Outils MCP disponibles (`wfl`)

- `mcp__wfl__wfl_get_grammar` : retourne le schéma JSON WFL v0.1 et la liste des workflows enregistrés.
- `mcp__wfl__wfl_validate` : valide la syntaxe YAML stricte et la sémantique (DAG, références, `sha256`).
- `mcp__wfl__wfl_save_draft` : enregistre un workflow valide en statut `draft` dans `.wf/` et SQLite WAL.
- `mcp__wfl__wfl_run` : exécute un workflow (`dry_run: true` par défaut ; obligatoire tant que `draft`).
- `mcp__wfl__wfl_seal` : scelle un workflow `draft` via son empreinte SHA-256 pour autoriser `dry_run: false`.
- `mcp__wfl__wfl_inspect_run` : inspecte le statut et les `step_logs` d'un `run_id`.
- `mcp__wfl__wfl_list_workflows` : liste tous les workflows enregistrés et leur statut (`draft` / `sealed`).

## Protocole obligatoire pour les agents

1. Appeler `wfl_get_grammar` si la grammaire WFL v0.1 n'est pas encore en contexte.
2. Rédiger le YAML WFL v0.1 et le vérifier avec `wfl_validate`.
3. Sauvegarder avec `wfl_save_draft`.
4. Simuler d'abord avec `wfl_run` (`dry_run: true`) et inspecter le résultat (`wfl_inspect_run` si besoin).
5. Sceller avec `wfl_seal` uniquement après validation / simulation réussie, puis lancer `wfl_run` (`dry_run: false`) pour l'exécution réelle.
6. Ne jamais modifier le fichier `.wf.yaml` sur disque après scellement sans repasser par `wfl_save_draft` + `wfl_seal` (le contrôle SHA-256 bloquera sinon l'exécution réelle).
