//! Tests d'intégration de l'API publique `atomic_patch`.
//!
//! Couvrent : match exact, fuzzy (CRLF/espaces), diagnostic d'erreur
//! (contexte fourni en cas d'échec/ambiguïté), rollback atomique
//! multi-fichiers quand la 2e opération échoue, preview de diff LCS
//! (insertion/suppression en milieu de fichier) et remplacement tolérant
//! à l'indentation (`apply_patch`) avec préservation des fins de ligne.

use std::fs;
use std::path::PathBuf;

use atomic_patch::{
    DiffLineKind, MatchStatus, PatchOperation, Transaction, TransactionOutcome, apply_patch,
    generate_unified_diff, match_block,
};

fn tmp_dir(tag: &str) -> PathBuf {
    let mut d = std::env::temp_dir();
    d.push(format!("atomic-patch-int-{}-{}", tag, std::process::id()));
    let _ = fs::create_dir_all(&d);
    d
}

fn cleanup(d: &std::path::Path) {
    let _ = fs::remove_dir_all(d);
}

/// 1) Match exact : un bloc littéral présent dans le source est localisé.
#[test]
fn integration_exact_match() {
    let source = "fn main() {\n    let x = 1;\n    let y = 2;\n}";
    let target = "let x = 1;";
    let r = match_block(source, target);
    assert_eq!(r.status, MatchStatus::Exact);
    assert!(
        r.position.is_some(),
        "un match exact doit exposer une position"
    );
}

/// 2) Fuzzy : source en CRLF + target légèrement différent → match approché.
#[test]
fn integration_fuzzy_crlf_spaces() {
    // Le source utilise des fins de ligne CRLF et le target contient une
    // ligne absente du source (donc pas de match exact du bloc entier),
    // mais au moins une ligne commune → match fuzzy.
    let source = "alpha\r\nbeta\r\ngamma";
    let target = "alpha\nbeta\nomega";
    let r = match_block(source, target);
    assert_eq!(r.status, MatchStatus::Fuzzy);
    assert!(r.position.is_some());
    assert!(
        r.context.is_some(),
        "un match fuzzy doit fournir un contexte de similarité"
    );
}

/// 3) Erreur (cible absente / ambiguë) : un contexte de diagnostic est fourni.
#[test]
fn integration_absent_or_ambiguous_provides_context() {
    let source = "a\nb\nc";
    let target = "zzz-not-here";
    let r = match_block(source, target);
    assert_eq!(r.status, MatchStatus::NotFound);
    assert!(r.position.is_none());
    let ctx = r
        .context
        .expect("le diagnostic doit être fourni en cas d'échec");
    assert!(!ctx.is_empty(), "le contexte ne doit pas être vide");
}

/// 4) Rollback atomique : la 2e opération échoue → la 1re est restaurée.
#[test]
fn integration_rollback_when_second_patch_fails() {
    let d = tmp_dir("int-rollback");
    let f1 = d.join("1.txt");
    // Parent absent → l'écriture du 2e fichier est impossible.
    let f2_bad = d.join("missing-dir").join("2.txt");
    fs::write(&f1, "one").unwrap();

    let t = Transaction::new()
        .add_op(PatchOperation {
            path: f1.clone(),
            new_content: "ONE".to_string(),
        })
        .add_op(PatchOperation {
            path: f2_bad,
            new_content: "TWO".to_string(),
        });

    let outcome = t.commit();
    match outcome {
        TransactionOutcome::RolledBack {
            failed_at,
            restored,
            ..
        } => {
            assert_eq!(failed_at, 1, "l'échec survient sur la 2e opération");
            assert_eq!(
                restored,
                vec![f1.clone()],
                "la 1re écriture est restaurée"
            );
        }
        TransactionOutcome::Committed { .. } => panic!("attendu RolledBack"),
    }

    // L'état initial du 1er fichier doit être intégralement restauré.
    assert_eq!(fs::read_to_string(&f1).unwrap(), "one");
    cleanup(&d);
}

/// 5) Preview de diff : le preview humain reflète les lignes ajoutées/retirées.
#[test]
fn integration_diff_preview() {
    let old = "one\ntwo\nthree";
    let new = "one\nTWO\nthree";
    let d = generate_unified_diff(old, new);

    // Le diff doit contenir des lignes ajoutées, retirées et en contexte.
    assert!(d
        .lines
        .iter()
        .any(|l| l.kind == DiffLineKind::Added));
    assert!(d
        .lines
        .iter()
        .any(|l| l.kind == DiffLineKind::Removed));
    assert!(d
        .lines
        .iter()
        .any(|l| l.kind == DiffLineKind::Context));

    // Le preview expose bien les marqueurs `+`/`-` et le contenu.
    assert!(d.preview.contains('+'), "le preview doit contenir des ajouts");
    assert!(
        d.preview.contains('-'),
        "le preview doit contenir des retraits"
    );
    assert!(
        d.preview.contains("one"),
        "le preview doit contenir le contexte"
    );
    assert!(
        d.preview.contains("TWO"),
        "le preview doit contenir la nouvelle ligne"
    );
}

/// 6) Stats : `Diff::stats()` retourne le bon nombre de lignes ajoutées/retirées.
#[test]
fn integration_diff_stats() {
    let old = "one\ntwo";
    let new = "one\nTWO\nthree";
    let d = generate_unified_diff(old, new);

    let (added, removed) = d.stats();
    assert_eq!(added, 2, "deux lignes ajoutées (TWO, three)");
    assert_eq!(removed, 1, "une ligne retirée (two)");
}

/// 7) LCS — insertion en milieu de fichier : `stats()` exact, suffixe en contexte.
///
/// Avec l'ancien diff linéaire, "three" était vu comme `Removed` + `Added`
/// (soit des stats de (3, 2)). Le LCS conserve "three" en `Context` : les
/// stats tombent exactement sur (2, 1).
#[test]
fn integration_lcs_mid_file_insert_stats() {
    let old = "one\ntwo\nthree";
    let new = "one\nTWO\nthree\nfour";
    let d = generate_unified_diff(old, new);

    assert_eq!(d.stats(), (2, 1), "TWO et four ajoutées, two retirée");

    let three = d
        .lines
        .iter()
        .find(|l| l.text == "three")
        .expect("la ligne 'three' doit figurer dans le diff");
    assert_eq!(
        three.kind,
        DiffLineKind::Context,
        "'three' ne doit ni être décalée ni marquée Removed+Added"
    );
    assert_eq!(three.old_line, Some(3), "inchangée côté old");
    assert_eq!(three.new_line, Some(3), "inchangée côté new");

    // Les numéros de ligne restent cohérents de bout en bout.
    for l in &d.lines {
        if let Some(n) = l.new_line {
            assert!((1..=4).contains(&n), "new_line hors bornes : {n}");
        }
        if let Some(n) = l.old_line {
            assert!((1..=3).contains(&n), "old_line hors bornes : {n}");
        }
    }
}

/// 8) LCS — suppression en milieu de fichier : seule la ligne retirée compte.
#[test]
fn integration_lcs_mid_file_delete_stats() {
    let old = "one\ntwo\nthree\nfour";
    let new = "one\nthree\nfour";
    let d = generate_unified_diff(old, new);

    assert_eq!(d.stats(), (0, 1), "'two' est la seule ligne retirée");

    let removed: Vec<&str> = d
        .lines
        .iter()
        .filter(|l| l.kind == DiffLineKind::Removed)
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(removed, vec!["two"]);

    // Le suffixe reste en contexte, avec des numéros recalculés.
    let three = d
        .lines
        .iter()
        .find(|l| l.text == "three")
        .expect("la ligne 'three' doit rester en contexte");
    assert_eq!(three.kind, DiffLineKind::Context);
    assert_eq!(three.old_line, Some(3));
    assert_eq!(three.new_line, Some(2));
}

/// 9) Remplacement tolérant : fichier indenté de 4 espaces, recherche sans
///    indentation → le bloc est retrouvé et `replace` ré-indenté à 4 espaces.
#[test]
fn integration_apply_patch_fuzzy_indent_shift_plus_four() {
    let source = "fn main() {\n    let x = 1;\n    let y = 2;\n}\n";
    let out = apply_patch(source, "let x = 1;\nlet y = 2;", "let z = 3;\nlet w = 4;")
        .expect("le fallback indentation doit retrouver le bloc");

    assert_eq!(out, "fn main() {\n    let z = 3;\n    let w = 4;\n}\n");
    assert!(
        out.contains("    let z = 3;"),
        "l'indentation de base (+4) doit être ré-appliquée"
    );
}

/// 10) Remplacement tolérant : l'indentation *relative* du bloc est préservée
///     (imbrication à 8 espaces dans le fichier, 4 dans la recherche).
#[test]
fn integration_apply_patch_fuzzy_preserves_relative_nesting() {
    let source = "fn main() {\n    if cond {\n        do_it();\n    }\n}\n";
    let out = apply_patch(
        source,
        "if cond {\n    do_it();\n}",
        "if other {\n    do_other();\n}",
    )
    .expect("bloc imbriqué attendu");

    assert_eq!(
        out,
        "fn main() {\n    if other {\n        do_other();\n    }\n}\n"
    );
}

/// 11) Remplacement tolérant : les fins de ligne CRLF du fichier sont
///     préservées, même quand la recherche est écrite en LF.
#[test]
fn integration_apply_patch_fuzzy_preserves_crlf() {
    let source = "fn main() {\r\n    let x = 1;\r\n    let y = 2;\r\n}\r\n";
    // Recherche en LF et sans indentation → passe 1 impossible, fallback fuzzy.
    let out = apply_patch(source, "let x = 1;\nlet y = 2;", "let z = 3;\nlet w = 4;")
        .expect("bloc retrouvé malgré CRLF + indentation");

    assert_eq!(
        out,
        "fn main() {\r\n    let z = 3;\r\n    let w = 4;\r\n}\r\n",
        "le CRLF doit être intégralement préservé"
    );
    assert_eq!(out.matches("\r\n").count(), 4, "4 fins de ligne CRLF");
    assert!(
        !out.replace("\r\n", "").contains('\n'),
        "aucun LF isolé ne doit subsister"
    );
}

/// 12) Refus sur match fuzzy ambigu : deux blocs équivalents après
///     normalisation d'indentation → `Ambiguous`, jamais de choix silencieux.
#[test]
fn integration_apply_patch_fuzzy_ambiguous_is_refused() {
    let source = "fn a() {\n    if cond {\n        body();\n    }\n}\n\
                  fn b() {\n    if cond {\n        body();\n    }\n}\n";
    let search = "if cond {\n    body();\n}";
    let err = apply_patch(source, search, "if z {\n    other();\n}")
        .expect_err("l'ambiguïté fuzzy doit être refusée");

    assert_eq!(err.status, MatchStatus::Ambiguous);
    assert!(err.is_error(), "Ambiguous doit être une erreur exploitable");
    assert!(err.position.is_none(), "aucune position ne doit être choisie");
    let ctx = err.context.expect("un contexte de diagnostic est requis");
    assert!(ctx.contains("ambigu"), "contexte explicite attendu : {ctx}");
}