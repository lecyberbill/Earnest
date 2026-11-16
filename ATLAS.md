# ATLAS - Earnest

## 1. Vision & Architecture
Écosystème agentique modulaire local-first sans dépendance cloud obligatoire.
- **Earnest Cockpit (Port 8080)** : Tableau de bord de supervision et d'orchestration agentique run-to-completion, gouverné par 10 invariants stricts validés en permanence (`tests/validate_invariants.py`).
- **Earnest IDE (Port 8090 & Desktop Tauri)** : Environnement de développement intégré professionnel haute performance (Rust natif + Monaco Editor officiel Zéro-Build).
- **Cycle de vie** : Éphémère (Run-to-completion, exit code 0) pour les agents, serveurs locaux à la demande.
- **Persistance** : SQLite (`~/.local/share/agent-manager/data.db`) avec moteur de recherche plein texte (FTS5) et table `memories`.
- **Ordonnancement** : Délégué à l'OS (`schtasks.exe` sous Windows, `cron` sous Linux/macOS).
- **UI** : Vanilla HTML5, CSS moderne natif (Grid/Flexbox/Nesting), Web Components (Custom Elements sans étape de build, 100% Zéro-Build).
- **Backend LLM & Routeur** : Routeur unifié multi-fournisseurs (DeepSeek, LM Studio, Ollama, OpenAI, vLLM) avec bascule automatique et gestion dynamique des clés API.

## 2. Topologie des Composants

### A. Earnest Cockpit (`earnest-core/` & `ui/`)
- `earnest-core/` : Binaire Rust natif central (`earnest-core.exe`, port 8080)
  - `src/db/` : Schéma SQLite, gestion des connexions WAL, persistance des sessions et mémoires.
  - `src/engine/` : Boucle ReAct run-to-completion et enregistrement des traces.
  - `src/llm/` : Client HTTP non-bloquant et routeur multi-modèles avec fallback.
  - `src/tools/` : Primitives fondamentales (`store_memory`, `recall_memory`, `sandbox_exec`, `register_tool`).
  - `src/server/` : Serveur HTTP minimaliste servant le Cockpit UI et l'API REST.
- `ui/` : Interface Web Cockpit Vanilla (HTML5/CSS3/ES6 sans npm ni bundler)
  - Composants : Console d'exécution en flux SSE, grille de mémoires, planificateur OS, tableau de bord de santé machine.

### B. Earnest IDE (`earnest-ide/`)
- `earnest-ide/core/` : Cœur serveur Rust natif (`earnest-ide-core.exe`, port 8090)
  - `src/engine/react.rs` : Moteur ReAct copilote avec support des sous-agents spécialisés (`invoke_subagent`).
  - `src/tools/` : Suite d'outils de développement avancés :
    - `fs` : Lecture paginée (`read_file`), écriture (`write_file`), édition chirurgicale (`edit_file`).
    - `search` : Recherche Regex multithreadée (`grep_search` via `ignore::WalkBuilder`) et arborescence (`list_files`).
    - `terminal` : Multi-terminaux interactifs sécurisés (`terminal_exec`).
    - `git` : Contrôle de source Git intégré (status, diff, commit).
    - `diagnostics` : Analyse statique et linter en temps réel (`cargo check`).
    - `memories` : Mémoire de projet RAG et injection automatique dans le prompt ReAct.
- `earnest-ide/ui/` : Interface IDE professionnelle Zéro-Build
  - **Monaco Editor** : Bibliothèque Monaco complète officielle AMD intégrée en statique avec prise en charge Web Worker par Blob URL.
  - **Monaco Diff Editor** : Comparateur de code côte à côte.
  - **Panneaux & Vues** : Explorateur dynamique avec dossiers extensibles/repliables, recherche globale (`Ctrl+Shift+F`), palette de commandes (`Ctrl+P`), édition IA inline (`Ctrl+K`), panneau de préférences et gestionnaire de modèles CRUD.
- `earnest-ide/src-tauri/` : Wrapper application de bureau native ultra-légère (Rust + Webview2).

### C. Référentiel & Invariants
- `invariants.json` : Spécification formelle des 10 règles fondamentales (INV-01 à INV-10).
- `tests/validate_invariants.py` : Script de certification continue garantissant la conformité stricte à 100%.



### D. Crates Utilitaires Zéro-Dépendance intégrés à `earnest-ide/core` (2026-11-15 → 2026-11-17)
- `context-trimmer` (`D:\Projet\context-trimmer`) : Troncature bornée des sorties d'outils (`truncate_tool_output`, limite 16 000 chars) et gestion du budget de contexte dans `react.rs`.
- `tool-output-cache` (`D:\Projet\tool-output-cache`) : Cache LRU persistant (`OnceLock`, `ToolCache::new(128)`) pour `read_file`, `list_files`, `grep_search`, avec normalisation des chemins et invalidation ciblée sur `write_code`, `edit_file`, `write_file`.
- `retry-budget` (`D:\Projet\retry-budget`) : Circuit-breaker à fenêtre glissante (`RetryBudget::new(10, 5)`) protégeant les appels `LLMRouter::chat_completion_with_fallback`.
- `json-repair` (`D:\Projet\json-repair`) : Réparation chirurgicale et totale de JSON malformés/tronqués (BOM, fences Markdown, extraction hors prose, automate LIFO, validateur strict RFC 8259 itératif jusqu'à 100k niveaux). Intégré dans `earnest-ide/core/src/llm/client.rs` (`parse_tool_args_with_repair`).

### E. Optimisations du Moteur ReAct & Gouvernance (`earnest-ide/core/src/engine/react.rs`, `governance/shadow.rs`, `llm/client.rs`)
- **Mode 3 (Spec-First)** : Outil `add_invariant` (formulaire plat) + auto-healing dans `submit_invariants` (conversion automatique des `probes` chaînes vers `ProbeSpec`, normalisation `runner`/`probe_id`).
- **Mode 2 (Shadow Worktree)** : `write_code` supporte le mode multi-fichiers atomique (`files[]`), `stage_only: true`, la validation directe sans patch, et l'édition ciblée (`target` + `replacement`) avec lecture prioritaire dans `shadow_worktree` ; synchronisation `git status --porcelain -uall` (nouveaux dossiers `src/`/`tests/` inclus) et fusion complète des fichiers stagés.
- **Streaming & Résilience** : Streaming SSE complet (Ollama + OpenAI/DeepSeek) avec `CANCEL_LLM`, sélecteur dynamique d'effort (`#chat-effort-select`), bouton Stop (`#btn-agent-stop`), `empty_tool_nudges` consécutif (réinitialisé à chaque appel d'outil), pré-chargement tour 0 (`!has_history`), exécution parallèle read-only (`std::thread::scope`), détection `[ANTI-LOOP]`, `/recall` et `recall_recent_runs`.
