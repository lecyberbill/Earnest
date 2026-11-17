# BENCHMARK & POST-MORTEM — `earnest-core` MCP & Runs Autonomes (`DeepSeek` vs `Qwen3.8` vs `LFM 3`)

Date : 2026-11-17

---

## 1. Nouveaux Composants & Correctifs sur `earnest-core`

| Commit | Composant | Apport |
|--------|-----------|--------|
| `f411db6` | `llm/client.rs`, `engine/react.rs` | `parse_tool_args_with_repair` (`json-repair` sur OpenAI & Ollama), `GLOBAL_TOOL_CACHE` (`OnceLock<Mutex<ToolCache(128)>>`) avec invalidation sur écriture/mémoire, compaction `compact_messages` (`60 000` chars) |
| `66b4bc9` | `mcp/mod.rs`, `main.rs` | Serveur **MCP JSON-RPC 2.0 stdio** (`--mcp`) exposant 5 outils : `earnest_run`, `earnest_probe_models`, `earnest_store_memory`, `earnest_recall_memory`, `earnest_list_runs` |
| `bf79316` | `tools/web.rs`, `tools/artifacts.rs`, `llm/client.rs`, `engine/react.rs` | `web_search` migré de DuckDuckGo Lite (HTTP 202 anti-bot) vers **Bing RSS + Wikipedia API**, troncature UTF-8 sûre dans `fetch_web_page`, `"think": false` + timeout 300 s sur Ollama, `mirrored_path` dans `create_artifact`, `--max-steps 24` + recrédit automatique `+8`, `[ANTI-STALL]` (`looks_like_unfinished_thought`), `--provider` |
| `c675e59` | `engine/react.rs` | `detect_runtime_environment()` : capture dynamique et injection dans le prompt système de l'OS, version OS, architecture, shell d'exécution (`PowerShell` vs `sh/bash`), runtimes disponibles (`Python`, `Cargo`, `Git`) et `CWD` |

Validation unitaire : `cargo test --manifest-path earnest-core\Cargo.toml` → **8/8 tests verts**.

---

## 2. Benchmark Comparatif — Mission Autonome Réelle (Inventaire `G:\` + Web + 30 Fiches HTML)

Consigne identique : scanner le disque `G:\` (~928 Go de modèles IA), rechercher les caractéristiques techniques sur Internet, générer `index.html` + une fiche HTML dédiée par modèle dans `D:\Projet\Hernest\projects_test\models_g_html\`, et vérifier les fichiers sur disque.

| Modèle / Backend | Run ID | Étapes | Tokens | Durée | `web_search` | Livrable réel sur disque | Verdict |
|---|---|---:|---:|---:|---|---|---|
| **DeepSeek Cloud (`deepseek-flash`)** | `run_06019003454c` | 67 | 91 391 | **1 min 48 s** | Ancien DDG (0 rés.) | Script Python 533 lignes complet (bloqué au dernier tour par `KeyError: 'vram'` faute de budget à 8 tours avant `bf79316`) | ⚡ Rapide, code propre |
| **Ollama (`qwen3.8:27b`)** | `run_73feac8cfee5` | 129 | 356 861 | **30 min 33 s** | Ancien DDG (0 rés.) | **30/30 HTML complets + auto-vérifiés** (0 lien mort, 0 orphelin ; sauvé par `[ANTI-STALL]` et recrédit `+8` tours) | ✅ **Fiable en autonomie longue** *(avant `think: false`)* |
| **LM Studio (`LFM 3` / `local-model`)** | `run_13fed053fd07` | 39 | 271 807 | **17 min 52 s** | **Bing RSS + `web_fetch` OK** | **1 seul squelette `index.html` (495 octets)** — aucune fiche générée malgré une conclusion affirmant le contraire | ❌ **Hallucination de complétion** sur tâche longue |

---

## 3. Enseignements Clés

1. **`web_search` DuckDuckGo bloquait silencieusement (`HTTP 202`) :** les agents recevaient 0 résultat. Le passage à **Bing RSS + Wikipedia API** a immédiatement rétabli la recherche web réelle (vérifié en live sur `LFM 3`).
2. **Le recrédit automatique (`+8` tours) et `[ANTI-STALL]` sont vitaux en autonomie :** sans eux, `qwen3.8:27b` se coupait au premier message d'annonce (`Run completed` prématuré) ou juste avant la vérification finale.
3. **Contexte système capturé dynamiquement > règles en dur :** injecter `OS`, `version`, `Shell`, `Runtimes` et `CWD` réels évite les commandes Unix (`find`) sous Windows PowerShell sans polluer le prompt avec des règles statiques.
4. **Limites des petits modèles (`LFM 3`) :** adaptés aux tâches courtes et ciblées, mais inadaptés à l'orchestration autonome longue (risque élevé d'hallucination de fin sans avoir exécuté les écritures). Pour des runs autonomes sans supervision, privilégier **`qwen3.8:27b`** (local) ou **`deepseek-flash`** (cloud).
