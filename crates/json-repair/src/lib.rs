//! # json-repair
//!
//! Crate de réparation **chirurgicale, totale et déterministe** de documents JSON
//! malformés typiques des sorties de LLM : JSON tronqué, entouré de prose,
//! enveloppé de fences Markdown, contenant des échappements cassés ou des
//! membres d'objet incomplets.
//!
//! ## Principes
//!
//! - **Totalité** : [`repair`] ne peut jamais échouer, jamais paniquer, même
//!   sur de l'UTF-8 arbitraire, des chaînes non fermées ou des entrées de
//!   profondeur 100 000.
//! - **Non-régression** : si l'entrée est déjà un JSON valide, la sortie est
//!   **strictement identique byte à byte** (aucune re-sérialisation, aucun
//!   réordonnancement de clés).
//! - **Zéro dépendance** : aucun crate tiers, pas de `serde_json`.
//! - **Zéro `unsafe`** : code 100 % sûr.
//! - **Zéro I/O** : pas de `fs`, pas de `std::env`, pas de réseau.
//!
//! ## Pipeline de réparation
//!
//! ```text
//!   BOM U+FEFF
//!       │
//!       ▼
//!   suppression des fences Markdown (``` / ~~~) ancrés en début de ligne
//!       │
//!       ▼
//!   trim externe (espaces, tabulations, retours à la ligne)
//!       │
//!       ▼
//!   si JSON déjà valide ──► RETOUR IMMEDIAT byte à byte
//!       │
//!       ▼
//!   extraction de la charge utile : premier `{` ou `[` hors chaîne,
//!   jusqu'à la fermeture correspondante (la prose est éliminée)
//!       │
//!       ▼
//!   si charge utile déjà valide ──► RETOUR
//!       │
//!       ▼
//!   scan structurel : parité backslashes, virgules différées,
//!   complétion littéraux/nombres/membres, fermeture LIFO
//!       │
//!       ▼
//!   String
//! ```
//!
//! ## Exemple
//!
//! ```
//! use json_repair::{repair, is_valid, validate, ErrorKind};
//!
//! let brut = r#"Voici la réponse :
//! ```json
//! {"nom": "Alice", "age": 30, "ville": "Paris"}
//! ```"#;
//!
//! let reponse = repair(brut);
//! assert!(is_valid(&reponse));
//!
//! let ok = r#"{"a": [1, 2, 3], "b": null}"#;
//! assert_eq!(repair(ok), ok, "un JSON valide doit ressortir inchangé");
//!
//! assert!(validate(brut).is_err());
//! let err = validate(brut).unwrap_err();
//! // La prose en tête n'est pas un JSON valide : le premier octet inattendu
//! // est détecté avant toute notion de contenu résiduel.
//! assert_eq!(err.kind, ErrorKind::UnexpectedByte);
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![allow(clippy::all)]

mod fence;
mod extract;
mod scan;
mod validate;

pub use validate::{is_valid, validate, ErrorKind, ValidationError};

// ---------------------------------------------------------------------------
// API publique
// ---------------------------------------------------------------------------

/// Répare une chaîne JSON potentiellement malformée.
///
/// **Total** : cette fonction ne peut jamais retourner d'erreur et ne
/// panique jamais, quelle que soit l'entrée (UTF-8, chaîne non fermée,
/// profondeur arbitraire, prose, fences Markdown).
///
/// Si l'entrée est déjà un JSON valide, la sortie est strictement identique
/// byte à byte (principe de non-régression : aucune re-sérialisation).
///
/// Pipeline : BOM → fences Markdown → trim → valid? → extraction → scan.
pub fn repair(input: &str) -> String {
    // 1. BOM
    let input = fence::strip_bom(input);

    // 2. Fences Markdown
    let input = fence::strip_markdown_fences(input);

    // 3. Trim externe
    let input = input.trim();

    // 4. Si déjà valide : retour immédiat byte à byte
    if is_valid(input) {
        return input.to_string();
    }

    // 5. Extraction de la charge utile JSON (premier conteneur hors chaîne)
    match extract::extract_payload(input) {
        Some(payload) => {
            let payload = payload.trim();
            if is_valid(payload) {
                return payload.to_string();
            }
            // Scan structurel complet (réparation chirurgicale)
            let repaired = scan::repair_structural(payload);
            // Filet de sécurité : `repair` est TOTAL et doit toujours produire
            // un JSON valide. Si la réparation chirurgicale n'y parvient pas
            // (ex: profondeur > MAX_DEPTH, cas hors classe), on retombe sur
            // `null` — valeur JSON toujours valide.
            if is_valid(&repaired) {
                repaired
            } else {
                "null".to_string()
            }
        }
        None => {
            // Pas de conteneur JSON trouvé : prose seule, entrée vide, etc.
            "null".to_string()
        }
    }
}

/// Répare des bytes d'entrée (UTF-8 avec remplacement).
///
/// Les séquences d'octets non valides sont remplacées par le caractère U+FFFD
/// (caractère de substitution Unicode) avant d'être passées à [`repair`].
pub fn repair_bytes(input: &[u8]) -> String {
    repair(&String::from_utf8_lossy(input))
}

// ---------------------------------------------------------------------------
// Tests — Brique 1 (validateur + API + invariants statiques)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// C3-P1 — TOTALITE : repair ne panique jamais sur des entrées hostiles.
    #[test]
    fn totality_never_panics_on_hostile_inputs() {
        let _ = repair(r#"{"nom": "Alice", "bio": "je dis \"bonjour\""#);
        let _ = repair(r#"{"a": 1\b"{"b""#);
        let deep = "[".repeat(50_000) + &"]".repeat(50_000);
        let _ = repair(&deep);
        let _ = repair("bonjour le monde");
        let _ = repair("");
        let _ = repair("\u{feff}");
        let _ = repair("```\n```");
        let _ = repair("{\"é\": \"à\"}");
        let _ = repair("{\"a\": \"x\u{0}y\"}");
    }

    /// C4-P1 — JSON déjà valide : sortie strictement identique byte à byte.
    #[test]
    fn valid_input_unchanged_byte_identical() {
        let samples = vec![
            r#"{"a": 1, "b": [2, 3], "c": null, "d": true}"#,
            r#"[1, 2, 3, [4, [5, [6]]]]"#,
            r#"null"#,
            r#"true"#,
            r#"false"#,
            r#"42"#,
            r#"3.14"#,
            r#"-0.01e10"#,
            r#""""#,
            "\n\t[1,2]  \r\n",
            r#"{"unicode": "\u00e9\u4e16\u20ac"}"#,
        ];
        for s in samples {
            assert!(is_valid(s), "attendu valide: {s:?}");
            let out = repair(s);
            assert_eq!(out, s.trim(), "byte-identical violé pour: {s:?}");
        }
    }

    /// C5-P1 — IDEMPOTENCE : repair(repair(x)) == repair(x).
    #[test]
    fn idempotency_repair_twice_is_stable() {
        let inputs = vec![
            r#"{"a": 1, "b": 2,"#,
            r#"{"a": [1, 2,"#,
            r#"{"a": 1"#,
            r#"[1, 2,"#,
            "bonjour\n```json\n{\"x\": 1\n```\nmerci",
            r#"{"s": "val\""#,
            r#"{"a": tru"#,
            r#"{"a": nu"#,
            r#"{"a": fal"#,
            r#"{"a": 3.1e"#,
        ];
        for s in inputs {
            let once = repair(s);
            let twice = repair(&once);
            assert_eq!(once, twice, "idempotence violée pour: {s:?} → once={once:?} twice={twice:?}");
            assert!(is_valid(&once), "sortie non valide pour: {s:?} → {once:?}");
        }
    }

    /// C5-P1 — DETERMINISME : même entrée → même sortie.
    #[test]
    fn determinism_same_input_same_output() {
        let s = r#"{"a": 1, "b": [2, 3,"#;
        let a = repair(s);
        let b = repair(s);
        assert_eq!(a, b);
    }

    /// C11-P1 — Profondeur 100 000.
    #[test]
    fn deep_nesting_100_000_validateur_iteratif_ne_panic_pas() {
        let depth = 100_000;
        let deep = "[".repeat(depth) + &"]".repeat(depth);

        let err = validate(&deep).unwrap_err();
        assert_eq!(err.kind, ErrorKind::DepthOutOfRange);

        let out = repair(&deep);
        assert!(is_valid(&out));
    }

    /// validate() — erreurs attendues.
    #[test]
    fn validate_detects_expected_errors() {
        let err = validate(r#"{"a": "unclosed"#).unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnexpectedEof);

        let err = validate("{} extra").unwrap_err();
        assert_eq!(err.kind, ErrorKind::TrailingContent);

        let err = validate("[1.2.3]").unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidNumber);

        let err = validate(r#"["\q"]"#).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidEscape);

        let err = validate("[1 & 2]").unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnexpectedByte);

        assert!(validate(r#"{"a": 1, "b": [2, 3]}"#).is_ok());
        assert!(validate("[1, 2, 3]").is_ok());
        assert!(validate("null").is_ok());
        assert!(validate("true").is_ok());
        assert!(validate("-1.5e10").is_ok());
    }

    /// is_valid() — cohérence.
    #[test]
    fn is_valid_is_consistent_with_validate() {
        assert_eq!(is_valid("{}"), true);
        assert_eq!(is_valid("{} x"), false);
        assert_eq!(is_valid("[1,]"), false);
        assert_eq!(is_valid(""), false);
        assert_eq!(is_valid("  "), false);
    }

    // -------------------------------------------------------------------------
    // C9-P1 — FENCE / PROSE / MARKDOWN
    // -------------------------------------------------------------------------

    /// C9 — Fence Markdown simple : le JSON est extrait et validé.
    #[test]
    fn fence_prose_markdown_simple_json_block() {
        let input = "```json\n{\"a\": 1}\n```";
        let result = repair(input);
        assert!(is_valid(&result));
        assert_eq!(result, r#"{"a": 1}"#);
    }

    /// C9 — Fence avec prose autour : la prose est éliminée, seul le JSON reste.
    #[test]
    fn fence_prose_markdown_with_surrounding_prose() {
        let input = "Voici la réponse :\n```json\n{\"nom\": \"Alice\", \"age\": 30}\n```\nMerci !";
        let result = repair(input);
        assert!(is_valid(&result));
        assert_eq!(result, r#"{"nom": "Alice", "age": 30}"#);
    }

    /// C9 — Fence tilde : même comportement que backtick.
    #[test]
    fn fence_prose_markdown_tilde_block() {
        let input = "~~~json\n[1, 2, 3]\n~~~";
        let result = repair(input);
        assert!(is_valid(&result));
        assert_eq!(result, "[1, 2, 3]");
    }

    /// C9 — Prose seule (pas de JSON) : retourne "null".
    #[test]
    fn fence_prose_markdown_prose_only_returns_null() {
        let result = repair("bonjour le monde, ceci est de la prose");
        assert_eq!(result, "null");
    }

    /// C9 — BOM + fence : les deux sont traités.
    #[test]
    fn fence_prose_markdown_bom_and_fence() {
        let input = "\u{feff}```json\n{\"x\": true}\n```";
        let result = repair(input);
        assert!(is_valid(&result));
        assert_eq!(result, r#"{"x": true}"#);
    }

    /// C9 — Fence sans info-string : fonctionne.
    #[test]
    fn fence_prose_markdown_no_info_string() {
        let input = "```\n{\"a\": [1, 2]}\n```";
        let result = repair(input);
        assert!(is_valid(&result));
        assert_eq!(result, r#"{"a": [1, 2]}"#);
    }

    /// C9 — Fence non fermé : le contenu est quand même extrait.
    #[test]
    fn fence_prose_markdown_unclosed_fence() {
        let input = "```json\n{\"a\": 1}";
        let result = repair(input);
        assert!(is_valid(&result));
        assert_eq!(result, r#"{"a": 1}"#);
    }

    /// C9 — JSON valide entouré de prose (sans fence) : extrait.
    #[test]
    fn fence_prose_markdown_valid_json_in_prose() {
        let input = "Le résultat est {\"ok\": true} comme prévu.";
        let result = repair(input);
        assert!(is_valid(&result));
        assert_eq!(result, r#"{"ok": true}"#);
    }

    /// C9 — JSON invalide dans un fence : réparé par le scan structurel.
    #[test]
    fn fence_prose_markdown_invalid_json_in_fence() {
        let input = "```json\n{\"a\": 1,\n```";
        let result = repair(input);
        assert!(is_valid(&result));
        assert_eq!(result, r#"{"a": 1}"#);
    }

    /// C9 — Multiple fences : seul le premier est traité.
    #[test]
    fn fence_prose_markdown_multiple_fences() {
        let input = "```json\n{\"a\": 1}\n```\ntext\n```json\n{\"b\": 2}\n```";
        let result = repair(input);
        assert!(is_valid(&result));
    }

    /// C9 — Entrée vide après nettoyage : "null".
    #[test]
    fn fence_prose_markdown_empty_after_cleaning() {
        assert_eq!(repair(""), "null");
        assert_eq!(repair("   "), "null");
        assert_eq!(repair("```"), "null");
    }

    /// C9 — Fence indenté (<= 3 espaces) : reconnu.
    #[test]
    fn fence_prose_markdown_indented_fence() {
        let input = "   ```json\n{\"a\": 1}\n   ```";
        let result = repair(input);
        assert!(is_valid(&result));
        assert_eq!(result, r#"{"a": 1}"#);
    }
}