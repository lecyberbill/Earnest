# TECHNICAL_REPORT - Earnest

## Contexte
Projet : Assistant Agentique Modulaire Local-First (Sans Démon).
Date de gel des invariants : 2026-11-03
Hash de gel : `3e9040714380fb182237bc698e4bc5cb3790186e613449e4955866fb5db6be79`

## État des Invariants (Gelé & Vérifié)
| ID | Statut | Sonde exécutée | Verdict |
|---|---|---|---|
| INV-01-SCHEMA-SQLITE | OK | `python -c "..."` vérification des 6 tables | OK |
| INV-02-NO-DAEMON | OK | `python -m earnest.cli run ... --test-dry-run` | OK |
| INV-03-CORE-TOOLS-SCHEMA | OK | `python -c "..."` 4 primitives validées | OK |
| INV-04-STORE-RECALL-MEMORY-ISOLATION | OK | `pytest tests/test_memory_primitives.py -q` | OK |
| INV-05-SANDBOX-EXECUTION | OK | `pytest tests/test_sandbox.py -q` | OK |
| INV-06-REGISTER-TOOL-PERSISTENCE | OK | `pytest tests/test_tool_registration.py -q` | OK |
| INV-07-EXPLICIT-FAILURE | OK | `pytest tests/test_explicit_failure.py -q` | OK |
| INV-08-NO-UNAUTHORIZED-EXTERNAL-PROCESS | OK | `python -c "..."` audit regex de non-contournement | OK |
| INV-09-ZERO-BUILD-VANILLA-UI | OK | `python -c "..."` vérification fichiers vanilla UI | OK |
| INV-10-OS-SCHEDULER-DELEGATION | OK | `pytest tests/test_os_scheduler.py -q` | OK |

## Changelog
- 2026-11-03 : Initialisation et gel de `invariants.json` (`3e9040714380fb182237bc698e4bc5cb3790186e613449e4955866fb5db6be79`).
- 2026-11-03 : Implémentation du schéma SQLite et de la couche persistance (`earnest/db/`).
- 2026-11-03 : Implémentation des primitives agentiques (`store_memory`, `recall_memory`, `sandbox_exec`, `register_tool`).
- 2026-11-03 : Implémentation de la sandbox confinée multi-runtimes (`earnest/tools/sandbox.py`).
- 2026-11-03 : Implémentation de l'ordonnanceur délégué à l'OS (`schtasks.exe` et `crontab`).
- 2026-11-03 : Implémentation du client LLM unifié et du moteur ReAct Run-to-Completion (`earnest/engine/react.py`).
- 2026-11-03 : Implémentation de l'interface cockpit Vanilla Web Components dans `ui/`.
- 2026-11-03 : Validation intégrale de la suite de 13 tests unitaires et des 10 sondes d'invariants.

- 2026-11-15 : Intégration de `context-trimmer` (`truncate_tool_output`), `tool-output-cache` (`ToolCache` LRU + invalidation sur écriture) et `retry-budget` (`RetryBudget` circuit-breaker) dans `earnest-ide/core`.
- 2026-11-15 : Améliorations `ReActEngine` : `write_code` multi-fichiers (`files[]`) et édition ciblée (`target`/`replacement`), pré-chargement tour 0, exécution parallèle read-only (`std::thread::scope`), détection `[ANTI-LOOP]`, et auto-healing `submit_invariants` / `add_invariant`.
- 2026-11-15 : Benchmarks Qwen 3.8 documentés dans `docs/BENCHMARK_QWEN38_CONTEXT_TRIMMER.md`, `docs/BENCHMARK_QWEN38_TOOL_OUTPUT_CACHE.md` et `docs/BENCHMARK_QWEN38_RETRY_BUDGET.md` (`cargo test` 5/5 sur `earnest-ide-core`).
- 2026-11-17 : Livraison de `json-repair` v0.1.0 (Mode 2 Contraint, 12/12 contrats `C1`–`C12`, 86/86 tests) et intégration dans `earnest-ide/core/src/llm/client.rs` (`parse_tool_args_with_repair`).
- 2026-11-17 : Durcissement complet d'Earnest IDE suite au stress-test `json-repair` : streaming SSE (`2b6df22`), `ToolCache` persistant + `/recall` (`c04e65e`), UI effort/Stop (`013b171`), `empty_tool_nudges` consécutif (`f81c82e`), `git status --porcelain -uall` (`a9565ca`), `resolve_patch` prioritaire sur `shadow_worktree` (`61b8a14`), validation sans patch et fusion complète des fichiers stagés (`16dcbba`). Bilan et tests réels de robustesse (5 000 entrées fuzzées + 1 Mo en 33,78 ms) consignés dans `docs/BENCHMARK_JSON_REPAIR.md` (`cargo test` 12/12 sur `earnest-ide-core`).
- 2026-11-17 : Portage dans `earnest-core` (agents autonomes) : `parse_tool_args_with_repair` via `json-repair` dans `earnest-core/src/llm/client.rs`, cache persistant `OnceLock<Mutex<ToolCache>>` avec invalidation et compaction `compact_messages` (60 000 chars) dans `earnest-core/src/engine/react.rs` (`cargo test` 3/3 sur `earnest-core`).
