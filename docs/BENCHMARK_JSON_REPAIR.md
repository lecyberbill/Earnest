# BENCHMARK & POST-MORTEM — `json-repair` + Durcissement Earnest IDE

Date : 2026-11-17
Modèle testé en conditions réelles : **DeepSeek Cloud (`deepseek-chat`)** en **Mode 2 (Contraint par Invariants)** sur un projet vierge (`D:\Projet\json-repair`).

---

## 1. Résumé Exécutif

Le développement complet du crate **`json-repair` v0.1.0** (zéro dépendance, `#![forbid(unsafe_code)]`, zéro I/O) sous Earnest IDE a servi de **stress-test grandeur nature** du moteur ReAct, du `ShadowRunner` et du client LLM streaming.

- **Résultat `json-repair` :** 12/12 contrats scellés (`C1`–`C12`) validés, **86/86 tests verts** (66 unitaires + 18 intégration + 2 doctests), 0 warning.
- **Intégration dans `earnest-ide-core` :** `parse_tool_args_with_repair` dans `earnest-ide/core/src/llm/client.rs` (réparation automatique des arguments d'outils malformés sur OpenAI/DeepSeek et Ollama, tout en conservant le garde-fou `__truncated_error` sur `write_code` quand `finish_reason == "length"`).
- **Robustesse réelle (`tests/json_repair_robustness.rs`) :** 5 000 entrées fuzzées déterministes + 500 tranches d'octets corrompues + troncature à chaque frontière UTF-8 + payload de **1 027 816 octets (~1 Mo)** réparé et cross-validé par `serde_json` en **33,78 ms** (profil debug).

---

## 2. Correctifs Majeurs Débusqués sur Earnest IDE

Ce run intensif a permis d'identifier et de corriger 10 points critiques dans `Hernest` :

| Commit | Composant | Problème identifié | Correctif appliqué |
|--------|-----------|-------------------|--------------------|
| `2b6df22` | `llm/client.rs`, `router.rs` | Blocage UI sans streaming, `default_model` ignoré, pas de détection `finish_reason=length` | Streaming SSE complet (Ollama + OpenAI/DeepSeek), priorité `default_model`, support `/api/show` + `thinking`, annulation immédiate `CANCEL_LLM`, remontée `__truncated_error` |
| `cf8ee3b` | `governance/shadow.rs` | `sync_active_changes_to_shadow()` écrasait les fichiers déjà stagés (`stage_only`) | Exclusion des fichiers déjà sales/stagés dans `shadow_worktree` lors de la synchronisation |
| `c04e65e` | `engine/react.rs` | Cache perdu entre runs, pré-chargement tour 0 déclenché même en reprise, compaction trop tardive | `ToolCache(128)` persistant (`OnceLock`) avec normalisation des chemins, pré-chargement conditionné à `!has_history`, `/recall` + `recall_recent_runs`, `final_answer` persisté (`LIMIT 12`), compaction (`keep_tail=24`, `soft_limit=max(60_000, max_chars/2)`) |
| `013b171` | `ui/index.html`, `ui/js/app.js` | Pas de sélecteur dynamique d'effort ni de bouton Stop fixe, UI ralentie par les tokens SSE | `#chat-effort-select` dynamique (`Off/Low/Medium/Xhigh`), bouton `#btn-agent-stop` câblé sur `/api/ide/run/cancel`, batching `requestAnimationFrame` |
| `ded5707` | `earnest-core` | Crates utilitaires non câblés côté Cockpit | Intégration de `context-trimmer`, `tool-output-cache` et `retry-budget` dans `earnest-core` |
| `f81c82e` | `engine/react.rs` | `empty_tool_nudges` cumulé sur tout le run → coupait des runs sains à ~58 étapes (`Run completed` prématuré) | Réinitialisation `empty_tool_nudges = 0` à chaque appel d'outil (3 relances *consécutives* autorisées) + message `[ANTI-STALL]` généralisé |
| `f2c5dc2` | `engine/react.rs` | Prompt Mode 2 disait *"le shadow est réinitialisé à chaque write_code"*, forçant DeepSeek à réécrire tout le projet dans `files[]` (>24k chars → troncature) | Prompt Mode 2 corrigé : édition un fichier à la fois (`target`/`replacement` ou `stage_only: true`), conservation des fichiers sur disque |
| `16dcbba` | `engine/react.rs`, `governance/shadow.rs` | `write_code` sans `patch`/`target`/`files` écrasait `target_file` avec `""` ; les fichiers `stage_only` n'étaient pas fusionnés au succès | Validation directe du `shadow_worktree` sur appel vide + fusion de tous les fichiers sales/non-suivis du `shadow_worktree` lors du succès |
| `a9565ca` | `governance/shadow.rs` | **Bug critique nouveau projet :** `git status --porcelain` sans `-uall` retournait `?? src/` (dossier) au lieu de `?? src/lib.rs`, donc `is_file()` était `false` et **aucun fichier de `src/` ou `tests/` n'était copié dans `shadow_worktree`** | Passage à `git status --porcelain -uall` sur les 3 appels dans `shadow.rs` (avec exclusion de `target/`) |
| `61b8a14` | `engine/react.rs` | `resolve_patch` lisait toujours `workspace_root` au lieu de `shadow_worktree`, écrasant la 1re édition `target`/`replacement` lors d'une 2e édition sur le même fichier | Lecture prioritaire dans `shadow_worktree` puis fallback sur `workspace_root` |

---

## 3. Leçons Apprises (Design & Exploitation LLM)

1. **`git status --porcelain -uall` indispensable en sandbox** : sur un dépôt neuf avec des répertoires non suivis (`src/`, `tests/`), `git status` sans `-uall` masque les fichiers individuels et casse silencieusement la synchronisation incrémentale.
2. **Accumulation des patches ciblés (`target`/`replacement`)** : un modèle qui corrige 2 endroits dans un même fichier enchaîne souvent 2 appels `target`/`replacement`. La résolution du fichier source doit lire l'état stagé dans le `shadow_worktree`, jamais l'état disque antérieur.
3. **Compteurs anti-stall consécutifs, jamais globaux** : sur des tâches de 60+ étapes, un modèle peut faire 3 pauses « texte seul » espacées dans le temps. Un compteur global coupe le run en plein milieu.
4. **Ne pas laisser d'hypothèses obsolètes dans `.ide/atlas.md`** : si l'atlas du projet contient une vieille règle (ex. *"le shadow est réinitialisé"*), le modèle la relit à chaque reprise et reproduit l'ancien contournement coûteux.
5. **Réparation JSON ciblée des arguments d'outils** : `json-repair` permet de sauver instantanément les appels d'outils en lecture/diagnostic affectés par une virgule finale, une fence Markdown ou une fermeture manquante, tout en refusant volontairement de réparer un `write_code` tronqué par `finish_reason=length` (intégrité du code source avant tout).

---

## 4. Résultats des Tests Réels de Robustesse (`earnest-ide-core`)

Exécution de `cargo test --manifest-path earnest-ide\core\Cargo.toml -- --nocapture` :

| Suite | Tests | Résultat | Détails mesurés |
|-------|-------|----------|-----------------|
| `llm::client::tests` | 3 | ✅ 3/3 | Réparation fences/virgules (`read_file`), réparation troncature lecture (`grep_search`), rejet sécurisé `write_code` tronqué (`finish_reason=length`) |
| `governance_tests` | 5 | ✅ 5/5 | Schéma `invariants.json`, SHA-256 déterministe, `PolicyManager` Mode 1/2/3 |
| `json_repair_robustness` | 4 | ✅ 4/4 | Troncature sur **chaque** frontière UTF-8 d'un payload complexe, **5 000** entrées fuzzées + **500** octets corrompus cross-validés contre `serde_json`, préservation byte-identique, **1 027 816 octets (~1 Mo)** réparés + validés en **33,78 ms** |
| **Total `earnest-ide-core`** | **12** | **✅ 12/12** | **0 échec, 0 warning** |
