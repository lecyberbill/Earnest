# Rapport technique — validation sur preuve, rejeu, recherche, adoption (16 nov. 2026)

## Contexte
Remplacement de `auto_validate` (kb_propose) par une promotion sur preuve d'exécution (`kb_confirm`),
plus rejeu approuvé, recherche en trois étapes, péremption par segments de version, hook d'adoption.
Invariants : `tests/INVARIANTS.md` ; sondes : `tests/improvements.rs` (hachages : `tests/PROBES.sha256`).

## Sondes avant implémentation (état de référence = code d'origine sans `auto_validate`)
Le code d'origine seul ne compile pas dans `tests/common/mod.rs` (E0063 `auto_validate`).
Sortie brute des sondes sur l'état de référence (P01 passe : non-régression, le champ n'existe plus) :
```
test p08_tool_list_has_no_human_action ... FAILED
test p02_proposer_cannot_self_promote ... FAILED
test p03_two_other_sessions_promote ... FAILED
test p06_content_update_resets_confirmations ... FAILED
test p04_confirm_requires_evidence ... FAILED
test p05_ko_resets_confirmations ... FAILED
test p09_search_prefix ... FAILED
test p01_auto_validate_field_rejected ... ok
test p16_no_process_or_network_outside_repro ... FAILED
test p15_migration_old_schema ... FAILED
test p10_search_infix_and_miss ... FAILED
test p14_missing_runner_program_is_explicit_error ... FAILED
test p12_repro_attach_is_inert ... FAILED
test p13_revalidate_requires_approval ... FAILED
test p11_version_segments ... FAILED
test p07_review_and_human_validate ... FAILED
test p17_hook_post_tool_use_reminder ... FAILED
test p17_hook_stop_blocks_without_kb_check ... FAILED
test p17_hook_stop_passes_cases ... FAILED
thread 'p08_tool_list_has_no_human_action' (16468) panicked at tests\improvements.rs:329:5:
  left: 6
 right: 8
thread 'p02_proposer_cannot_self_promote' (9416) panicked at tests\improvements.rs:144:9:
thread 'p03_two_other_sessions_promote' (3364) panicked at tests\improvements.rs:167:5:
thread 'p06_content_update_resets_confirmations' (25020) panicked at tests\improvements.rs:67:10:
thread 'p04_confirm_requires_evidence' (16272) panicked at tests\improvements.rs:211:5:
thread 'p05_ko_resets_confirmations' (22980) panicked at tests\improvements.rs:67:10:
thread 'p09_search_prefix' (23048) panicked at tests\improvements.rs:344:5:
thread 'p16_no_process_or_network_outside_repro' (23144) panicked at tests\improvements.rs:572:5:
  left: []
 right: ["repro.rs"]
thread 'p15_migration_old_schema' (25300) panicked at tests\improvements.rs:531:5:
thread 'p10_search_infix_and_miss' (2136) panicked at tests\improvements.rs:355:5:
thread 'p14_missing_runner_program_is_explicit_error' (11904) panicked at tests\improvements.rs:434:5:
thread 'p12_repro_attach_is_inert' (20564) panicked at tests\improvements.rs:423:5:
thread 'p13_revalidate_requires_approval' (7492) panicked at tests\improvements.rs:434:5:
thread 'p11_version_segments' (27072) panicked at tests\improvements.rs:382:5:
  left: "a_revoir"
 right: "valide"
thread 'p07_review_and_human_validate' (26612) panicked at tests\improvements.rs:298:5:
  left: 2
 right: 0
thread 'p17_hook_post_tool_use_reminder' (26972) panicked at tests\improvements.rs:676:5:
  left: -196608
 right: 0
thread 'p17_hook_stop_blocks_without_kb_check' (24440) panicked at tests\improvements.rs:634:5:
  left: -196608
 right: 2
thread 'p17_hook_stop_passes_cases' (25424) panicked at tests\improvements.rs:652:5:
  left: -196608
 right: 0
test result: FAILED. 1 passed; 18 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.34s
error: test failed, to rerun pass `--test improvements`
```
Note : un premier passage avait lié une bibliothèque obsolète (répertoire `target` partagé avec la copie
de référence) ; il a été écarté et refait avec un répertoire `target` séparé.

## Après implémentation
- `cargo test --release -- --skip a14` : A1–A13, A15–A25 (24) OK ; P01–P17 (19 tests) OK ; 4 tests unitaires OK.
- A14 (10 000 entrées) : ÉCHEC préexistant. Référence : p95 recherche = 265 ms ; après : 225 ms (seuil 100 ms).
  Premier essai avec recherche par préfixe sur le chemin courant : > 1,4 s par recherche, corrigé par
  la recherche en trois étapes.
- Modifications de vérification (accord de l'utilisateur) : `tests/acceptance.rs` A4, 6 → 8 outils.
- Sonde P16 : la description de l'outil contenait le mot « revalidate » ; description reformulée (implémentation corrigée, sonde inchangée).
- clippy : `cargo clippy --all-targets -- -D warnings -A dead_code` OK (les helpers partagés de `tests/common` déclenchent `dead_code` dans le test `improvements`).

## Limites connues
- Un agent qui fabrique des preuves peut atteindre `min_confirmations` avec des sessions distinctes : d'où `validation_mode=auto` et `kb review`.
- Le hook d'adoption ne couvre que Claude Code ; il est en PowerShell (Windows).
