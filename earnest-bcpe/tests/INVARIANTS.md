# Invariants — évolutions « validation sur preuve », rejeu, recherche, adoption

Chaque invariant = un énoncé + une sonde exécutable (`cargo test --release --test improvements <nom>`)
+ le résultat attendu. Toutes les sondes ont été exécutées et ont échoué avant l'implémentation
(sortie brute dans `TECHNICAL_REPORT.md`). Les sondes marquées « non-régression » passent déjà.

| N° | Énoncé | Sonde | Résultat attendu |
| --- | --- | --- | --- |
| P01 | `kb_propose` refuse `auto_validate` (champ inconnu) et ne crée aucune entrée. | `p01_auto_validate_field_rejected` | `invalid_argument`, 0 ligne. |
| P02 | L'auteur d'une entrée ne peut pas la promouvoir, même avec N confirmations avec preuve (une session = un vote). | `p02_proposer_cannot_self_promote` | Statut `non_teste`, `validation_mode` NULL. |
| P03 | 2 sessions distinctes de l'auteur, chacune avec preuve, promeuvent l'entrée en `valide` / `validation_mode='auto'`, `validated_by` commence par `auto:` ; `kb_search` et `kb_check` exposent `validation_mode`. | `p03_two_other_sessions_promote` | Après 1 confirmation : `non_teste`. Après 2 : `valide`/`auto`. |
| P04 | Sans preuve valable (commande vide, extrait vide, code de sortie ≠ 0), `kb_confirm` échoue et ne change rien. | `p04_confirm_requires_evidence` | `invalid_argument`, `use_count` inchangé. |
| P05 | Un `ko` annule les confirmations antérieures : après `ko`, il faut 2 nouvelles confirmations. | `p05_ko_resets_confirmations` | `a_revoir` après 1 confirmation post-`ko`, `valide` après 2. |
| P06 | Une modification de contenu annule les confirmations de la version précédente. | `p06_content_update_resets_confirmations` | Confirmation d'une ancienne version non comptée. |
| P07 | `kb review` liste uniquement les entrées `valide`/`auto` ; `kb validate` (plusieurs identifiants) les passe en `humain`. | `p07_review_and_human_validate` | Liste puis liste vide, `validation_mode='humain'`. |
| P08 | Aucun outil MCP ne porte une action humaine ; les 8 outils attendus sont exposés. | `p08_tool_list_has_no_human_action` | 8 noms, aucun `validate/archive/delete/restore/edit`. |
| P09 | La recherche trouve par préfixe (`attend` → `attendre`). | `p09_search_prefix` | Entrée `attente-pause` renvoyée. |
| P10 | La recherche trouve par fragment (`ttent`) ; un terme absent reste « non couvert ». | `p10_search_infix_and_miss` | Entrée trouvée ; `xyzzy` : vide, `covered=false`. |
| P11 | `version_segments: 2` : `2.0` ≡ `2.0.5` (entrée inchangée), `2.1.0` ⇒ `a_revoir`. Défaut strict inchangé (A10). | `p11_version_segments` | Statuts comme décrits. |
| P12 | `kb_attach_repro` refuse un runner non déclaré ; une pièce jointe valide n'exécute rien et ne change pas le statut (négatif : aucun processus dans le chemin MCP). | `p12_repro_attach_is_inert` | `invalid_argument` ; 0 outcome, statut inchangé. |
| P13 | `kb revalidate` n'exécute un rejeu qu'après `kb approve-repro` humain ; rejeu réussi ⇒ `valide`/`auto`, version produit alignée ; rejeu en échec ⇒ `a_revoir`, `fail_count`+1, code de sortie ≠ 0. | `p13_revalidate_requires_approval` | Voir sonde. |
| P14 | Programme de runner introuvable : échec explicite (code ≠ 0, nom du programme dans la sortie), entrée inchangée. | `p14_missing_runner_program_is_explicit_error` | Code ≠ 0, statut inchangé. |
| P15 | Une base créée avec l'ancien schéma est migrée sans perte (colonnes, tables, index trigramme peuplé). | `p15_migration_old_schema` | Entrée conservée et retrouvée par `kb_search`. |
| P16 | Seul `src/core/repro.rs` lance un processus ; rien dans `src/mcp` ni ailleurs ne lance de processus ni n'ouvre de réseau. | `p16_no_process_or_network_outside_repro` | Un seul fichier avec `Command::new`. |
| P17 | Hook d'adoption : `Stop` bloque (code 2) si un script a été écrit sans `kb_check` ensuite ; passe sinon, ou si `stop_hook_active` ; `PostToolUse` rappelle `kb_check` pour un fichier script. | `p17_hook_*` (Windows) | Codes de sortie décrits. |
| NR | Non-régression : A1–A25 inchangés (sauf A4 : 6 → 8 outils, accord de l'utilisateur). | `cargo test --release --test acceptance` | Voir rapport. |
