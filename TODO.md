# TODO

## i18n / UI (Earnest IDE)
- [x] Traduire et passer par `i18n.js` les libellés français encore en dur (ex. `Approuver`, `Sceller`, `Valider et Sceller`, `Enregistrer`, etc.) afin que l'interface reste 100 % en anglais lorsque la langue EN est sélectionnée :
  - `earnest-ide/ui/index.html` (`btn-gov-approve`, etc.)
  - `earnest-ide/ui/js/app.js` (`btnGovApprove.textContent`, etc.)
  - `earnest-ide/ui/js/components/kb-view.js`
  - `earnest-ide/ui/js/i18n.js` (vérifier la couverture complète EN/FR)

## Tour de contrôle d'agent (`earnest-core` + `ui/`)
- [ ] Éprouver et enrichir la tour de contrôle (pipelines multi-agents Architect → Dev → QA, orchestration, métriques & benchmarks dédiés dans `docs/`)
- [ ] Intégrer les prochaines briques : `tool-guard`, compaction par budget de tokens (`context-trimmer`), et ordonnanceur DAG (`agent-scheduler`)

## Améliorations moteur ReAct (`earnest-ide/core`)
- [x] Ajouter `append: true` sur `write_code` et `write_file` pour permettre d'écrire les gros fichiers par morceaux (squelette puis ajouts)
- [x] Ajouter une relance anti-stall en Mode 1 (`Normal`) quand une réponse est tronquée en plein code sans appel d'outil



## Sécurité, Bac à sable & Dossier de sortie (`earnest-core` + `ui/`)
- [x] **Dossier de sortie des livrables** : défaut sur `Documents\Earnest`, configurable dans les Paramètres (`#setting-default-output-dir`) et surchargeable par run dans la Run Console (`#output-dir-input`).
- [x] **Mode Paranoïaque (bac à sable vs mode libre)** : option dans les Paramètres (`#setting-paranoid-mode`) qui bloque côté moteur Rust (`ReActEngine::dispatch_tool`) tout accès disque (`read_file`, `edit_file`, `write_file`, `list_dir`, `list_files`, `grep_search`, `symbol_search`, `workspace_map`) et terminal (`terminal_exec`).
- [x] **Déduplication des appels d'outils** : filtrage des appels identiques `(name, arguments)` au sein d'un même tour ReAct.
- [ ] **À creuser — VM Linux minimaliste avec snapshot/restauration** : exécuter l'agent en mode libre dans une VM Linux minimaliste dédiée avec snapshot avant chaque run pour pouvoir restaurer l'état à tout moment.
