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


