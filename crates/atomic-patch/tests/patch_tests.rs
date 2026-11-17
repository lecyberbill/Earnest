//! Tests d'intégration de l'API publique `atomic_patch`.
//!
//! Couvrent : match exact, fuzzy (CRLF/espaces), diagnostic d'erreur
//! (contexte fourni en cas d'échec/ambiguïté), rollback atomique
//! multi-fichiers quand la 2e opération échoue, et preview de diff.

use std::fs;
use std::path::PathBuf;

use atomic_patch::{
    DiffLineKind, MatchStatus, PatchOperation, Transaction, TransactionOutcome,
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