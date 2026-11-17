//! Extraction de la **charge utile JSON** d'une chaîne potentiellement enterrée
//! dans de la prose, des fences Markdown ou du bruit textuel.
//!
//! ## Algorithme
//!
//! 1. Trouver le **premier** `{` ou `[` **hors chaîne JSON** (en tenant compte
//!    de la parité des backslashes pour les guillemets échappés).
//! 2. À partir de ce caractère, **suivre les conteneurs imbriqués** jusqu'à la
//!    fermeture équilibrée du conteneur racine.
//! 3. Retourner la sous-tranche `[start..end]` (zéro copie).
//!
//! Si aucun conteneur JSON n'est trouvé, retourner `None` (l'appelant
//! produira alors `"null"`).
//!
//! ## Garanties
//!
//! - **Itératif** : pas de récursion, compatible avec des documents de
//!   profondeur arbitraire.
//! - **Zéro copie** : retourne une sous-tranche `&str` de l'entrée.
//! - **Total** : ne panique jamais sur de l'UTF-8 arbitraire.
//! - **Parité de backslashes** : un `"` n'est délimiteur que si le nombre
//!   de `\` précédents est pair.

/// Extrait la charge utile JSON d'une chaîne.
///
/// Retourne `Some(payload)` si un conteneur JSON (`{...}` ou `[...]`) est
/// trouvé hors chaîne, `None` sinon.
///
/// La recherche ignore les `{`/`[` présents **à l'intérieur** de chaînes JSON
/// (détection de parité de guillemets avec prise en compte des backslashes).
pub(crate) fn extract_payload(input: &str) -> Option<&str> {
    let bytes = input.as_bytes();
    let n = bytes.len();

    // Phase 1 : trouver le premier `{` ou `[` hors chaîne.
    let mut i: usize = 0;
    let mut in_string = false;
    let mut backslash_run = 0u32;
    let mut start: Option<usize> = None;

    while i < n {
        let b = bytes[i];
        match b {
            b'\\' => {
                backslash_run += 1;
            }
            b'"' => {
                // Un guillemet est un délimiteur si le nombre de backslashes
                // précédents (consécutifs) est pair.
                if backslash_run % 2 == 0 {
                    in_string = !in_string;
                }
                backslash_run = 0;
            }
            _ => {
                backslash_run = 0;
                if !in_string && (b == b'{' || b == b'[') {
                    start = Some(i);
                    break;
                }
            }
        }
        i += 1;
    }

    let start = start?;
    let open_byte = bytes[start]; // b'{' ou b'['
    let close_byte = match open_byte {
        b'{' => b'}',
        b'[' => b']',
        _ => unreachable!(), // garanti par le match ci-dessus
    };

    // Phase 2 : suivre les conteneurs imbriqués depuis `start`.
    // Pile explicite des types de conteneurs ouverts (itératif).
    let mut depth: i32 = 1;
    let mut i = start + 1;
    in_string = false;
    backslash_run = 0;

    while i < n {
        let b = bytes[i];
        match b {
            b'\\' => {
                backslash_run += 1;
            }
            b'"' => {
                if backslash_run % 2 == 0 {
                    in_string = !in_string;
                }
                backslash_run = 0;
            }
            _ => {
                backslash_run = 0;
                if !in_string {
                    if b == b'{' || b == b'[' {
                        depth += 1;
                    } else if b == b'}' || b == b']' {
                        depth -= 1;
                        if depth == 0 {
                            // Fermeture du conteneur racine trouvée.
                            return Some(&input[start..=i]);
                        }
                    }
                }
            }
        }
        i += 1;
    }

    // Phase 3 : conteneur non fermé (JSON tronqué).
    // On retourne quand même la sous-tranche jusqu'à la fin de l'entrée :
    // le scan structurel (Brique 3) s'occupera de fermer les conteneurs.
    let _ = close_byte;
    Some(&input[start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_simple_object() {
        assert_eq!(extract_payload(r#"{"a": 1}"#), Some(r#"{"a": 1}"#));
    }

    #[test]
    fn extract_simple_array() {
        assert_eq!(extract_payload("[1, 2, 3]"), Some("[1, 2, 3]"));
    }

    #[test]
    fn extract_object_in_prose() {
        let input = "Voici la réponse : {\"nom\": \"Alice\", \"age\": 30} Merci !";
        assert_eq!(
            extract_payload(input),
            Some(r#"{"nom": "Alice", "age": 30}"#)
        );
    }

    #[test]
    fn extract_array_in_prose() {
        let input = "Le résultat est [1, 2, 3] comme prévu.";
        assert_eq!(extract_payload(input), Some("[1, 2, 3]"));
    }

    #[test]
    fn extract_nested_containers() {
        let input = r#"prose {"a": [1, {"b": 2}]} prose"#;
        assert_eq!(extract_payload(input), Some(r#"{"a": [1, {"b": 2}]}"#));
    }

    #[test]
    fn extract_truncated_no_close() {
        // JSON tronqué : pas de fermeture. Retourne jusqu'à la fin.
        let input = r#"prose {"a": [1, 2"#;
        assert_eq!(extract_payload(input), Some(r#"{"a": [1, 2"#));
    }

    #[test]
    fn extract_no_json_returns_none() {
        assert_eq!(extract_payload("bonjour le monde"), None);
        assert_eq!(extract_payload(""), None);
        assert_eq!(extract_payload("   "), None);
    }

    #[test]
    fn extract_ignores_braces_in_strings() {
        // Les { et } dans les chaînes ne doivent pas être comptés.
        let input = r#"{"s": "text with { and } inside", "n": 42}"#;
        assert_eq!(extract_payload(input), Some(r#"{"s": "text with { and } inside", "n": 42}"#));
    }

    #[test]
    fn extract_escaped_quotes_in_strings() {
        // Les guillemets échappés ne doivent pas fermer la chaîne.
        let input = r#"{"s": "he said \"hello {world}\"", "n": 1}"#;
        assert_eq!(extract_payload(input), Some(r#"{"s": "he said \"hello {world}\"", "n": 1}"#));
    }

    #[test]
    fn extract_first_container_only() {
        // Si deux conteneurs sont présents, seul le premier est extrait.
        let input = r#"first {"a": 1} then [2, 3]"#;
        assert_eq!(extract_payload(input), Some(r#"{"a": 1}"#));
    }
}