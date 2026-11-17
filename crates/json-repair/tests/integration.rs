//! Tests d'intégration — C12 : scénarios LLM réalistes + invariants globaux.
//!
//! Chaque scénario reproduit une sortie de LLM typique (JSON tronqué, entouré
//! de prose, en fences Markdown, avec échappements cassés, etc.) et vérifie
//! que `repair` produit un JSON valide, idempotent et déterministe.

use json_repair::{is_valid, repair, repair_bytes};

/// Vérifie les invariants universels sur une entrée quelconque.
fn check_invariants(input: &str) -> String {
    let out = repair(input);
    assert!(
        is_valid(&out),
        "sortie non valide pour {input:?} → {out:?}"
    );
    let again = repair(&out);
    assert_eq!(out, again, "idempotence violée pour {input:?}");
    let det = repair(input);
    assert_eq!(out, det, "déterminisme violé pour {input:?}");
    out
}

#[test]
fn scenario_truncated_object() {
    let out = check_invariants(r#"{"nom": "Alice", "age": 30, "ville": "Par"#);
    assert_eq!(out, r#"{"nom": "Alice", "age": 30, "ville": "Par"}"#);
}

#[test]
fn scenario_truncated_array() {
    let out = check_invariants("[1, 2, 3, 4");
    assert_eq!(out, "[1, 2, 3, 4]");
}

#[test]
fn scenario_markdown_fence_with_prose() {
    let input = "Bien sûr ! Voici le résultat :\n```json\n{\"ok\": true, \"items\": [1, 2, 3]}\n```\nN'hésitez pas.";
    let out = check_invariants(input);
    assert_eq!(out, r#"{"ok": true, "items": [1, 2, 3]}"#);
}

#[test]
fn scenario_trailing_comma() {
    let out = check_invariants(r#"{"a": 1, "b": 2,}"#);
    assert_eq!(out, r#"{"a": 1, "b": 2}"#);
}

#[test]
fn scenario_unterminated_string() {
    let out = check_invariants(r#"{"message": "Bonjour tout le monde"#);
    assert_eq!(out, r#"{"message": "Bonjour tout le monde"}"#);
}

#[test]
fn scenario_truncated_literal() {
    let out = check_invariants(r#"{"flag": tru"#);
    assert_eq!(out, r#"{"flag": true}"#);
}

#[test]
fn scenario_truncated_number() {
    let out = check_invariants(r#"{"pi": 3.14"#);
    assert_eq!(out, r#"{"pi": 3.14}"#);
}

#[test]
fn scenario_deeply_nested_truncated() {
    let out = check_invariants(r#"{"a": {"b": {"c": [1, 2, {"d": 3"#);
    assert_eq!(out, r#"{"a": {"b": {"c": [1, 2, {"d": 3}]}}}"#);
}

#[test]
fn scenario_escaped_quotes() {
    let out = check_invariants(r#"{"quote": "Il a dit \"bonjour\""#);
    assert_eq!(out, r#"{"quote": "Il a dit \"bonjour\""}"#);
}

#[test]
fn scenario_prose_only() {
    let out = check_invariants("Je ne peux pas répondre à cette question.");
    assert_eq!(out, "null");
}

#[test]
fn scenario_valid_json_unchanged() {
    let input = r#"{"a": 1, "b": [2, 3], "c": null}"#;
    assert_eq!(repair(input), input);
}

#[test]
fn scenario_bom_and_fence() {
    let input = "\u{feff}```json\n{\"x\": 42}\n```";
    let out = check_invariants(input);
    assert_eq!(out, r#"{"x": 42}"#);
}

#[test]
fn scenario_repair_bytes_invalid_utf8() {
    // Octets non-UTF8 : remplacés par U+FFFD, pas de panique.
    let bytes = b"{\"a\": \xff\xfe}";
    let out = repair_bytes(bytes);
    assert!(is_valid(&out));
}

#[test]
fn scenario_empty_input() {
    assert_eq!(repair(""), "null");
    assert_eq!(repair("   \n\t "), "null");
}

#[test]
fn scenario_orphan_closer() {
    let out = check_invariants(r#"{"a": 1}]"#);
    assert_eq!(out, r#"{"a": 1}"#);
}

#[test]
fn scenario_mismatched_closer() {
    let out = check_invariants(r#"{"a": [1, 2}"#);
    assert_eq!(out, r#"{"a": [1, 2]}"#);
}

#[test]
fn scenario_deep_nesting_100k_no_panic() {
    let depth = 100_000;
    let deep = "[".repeat(depth) + &"]".repeat(depth);
    let out = repair(&deep);
    assert!(is_valid(&out));
}

#[test]
fn scenario_unicode_content() {
    let out = check_invariants(r#"{"ville": "São Paulo", "emoji": "🎉"}"#);
    assert!(is_valid(&out));
}