//! Scan structurel de réparation JSON.
//!
//! Parité de backslashes : un `"` n'est délimiteur de chaîne que si le nombre
//! de `\` consécutifs précédents est pair ; un backslash pendant en fin
//! d'entrée est supprimé (jamais de `\"` invalide, jamais de panic).
//!
//! Réparation chirurgicale d'un JSON tronqué/malformé : automate 3 états
//! (Normal | InString | InStringEscape), pile explicite d'ouvreurs, virgules
//! différées, complétion des littéraux/nombres tronqués, membres d'objet
//! incomplets, fermeture LIFO typée. Itératif, O(n), zéro allocation superflue.
//!
//! Couverture des contrats : C6 (fermeture LIFO), C7 (virgules terminales et
//! membres tronqués), C8 (parité des échappements), C10 (littéraux/nombres).

/// État interne d'un conteneur objet.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ObjState {
    /// Attend une clé (ou `}`).
    KeyOrClose,
    /// Attend `:` après une clé.
    Colon,
    /// Attend une valeur après `:`.
    Value,
    /// Attend `,` ou `}` après une valeur.
    CommaOrClose,
}

/// État interne d'un conteneur tableau.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArrState {
    /// Attend une valeur (ou `]`).
    ValueOrClose,
    /// Attend `,` ou `]` après une valeur.
    CommaOrClose,
}

/// Entrée de pile : type + état.
#[derive(Clone, Copy)]
enum Frame {
    Obj(ObjState),
    Arr(ArrState),
}

/// Répare structurellement une charge utile JSON (déjà extraite).
///
/// Retourne une `String` valide (au sens RFC 8259) autant que possible.
///
/// ## Virgules différées
///
/// Une virgule rencontrée n'est **pas** émise immédiatement : on lève le
/// drapeau `pending_comma`. Elle n'est matérialisée que si une nouvelle
/// clé/valeur suit réellement. Si l'entrée se termine (ou qu'un fermeur
/// arrive) avant, la virgule est simplement abandonnée → virgule terminale
/// supprimée sans heuristique de retour-arrière.
///
/// ## État « valeur consommée »
///
/// Après qu'une valeur (chaîne, conteneur, scalaire) est complète, l'état du
/// conteneur parent passe à `CommaOrClose`. C'est ce qui évite d'injecter un
/// `null` parasite à la fermeture : seul un parent encore en `Value`/`Colon`
/// (valeur réellement absente) déclenche la complétion.
pub(crate) fn repair_structural(input: &str) -> String {
    let bytes = input.as_bytes();
    let n = bytes.len();
    let mut out = String::with_capacity(n + 8);
    let mut stack: Vec<Frame> = Vec::new();
    let mut i = 0usize;

    // État de l'automate de chaîne.
    let mut in_string = false;
    let mut escape = false;

    // Suivi de la valeur scalaire en cours (littéral/nombre) pour complétion.
    let mut token = String::new();

    // Virgule différée : levée à la rencontre d'une `,`, matérialisée juste
    // avant la prochaine clé/valeur, abandonnée si l'entrée/le conteneur se
    // termine avant.
    let mut pending_comma = false;

    // Marque la valeur courante comme consommée dans le conteneur parent.
    fn mark_value_done(stack: &mut [Frame]) {
        if let Some(top) = stack.last_mut() {
            match top {
                Frame::Obj(ObjState::Value) => *top = Frame::Obj(ObjState::CommaOrClose),
                Frame::Arr(ArrState::ValueOrClose) => *top = Frame::Arr(ArrState::CommaOrClose),
                _ => {}
            }
        }
    }

    // Matérialise la virgule différée si elle est levée.
    fn emit_pending_comma(pending: &mut bool, out: &mut String) {
        if *pending {
            out.push(',');
            *pending = false;
        }
    }

    // Flush du token scalaire accumulé (littéral/nombre) avec complétion.
    // C10 : littéraux tronqués (true/false/null) et nombres tronqués.
    fn flush_token(token: &mut String, out: &mut String) {
        if token.is_empty() {
            return;
        }
        let t = token.as_str();
        // Littéraux tronqués : compléter si préfixe de true/false/null.
        let completed: Option<&str> = if "true".starts_with(t) {
            Some("true")
        } else if "false".starts_with(t) {
            Some("false")
        } else if "null".starts_with(t) {
            Some("null")
        } else {
            None
        };
        if let Some(lit) = completed {
            out.push_str(lit);
        } else {
            // Nombre : supprimer les suffixes invalides pendants (., e, E, +, -).
            let mut end = t.len();
            let tb = t.as_bytes();
            while end > 0 {
                let c = tb[end - 1];
                if c == b'.' || c == b'e' || c == b'E' || c == b'+' || c == b'-' {
                    end -= 1;
                } else {
                    break;
                }
            }
            if end == 0 {
                // Rien de valide : émettre null.
                out.push_str("null");
            } else {
                out.push_str(&t[..end]);
            }
        }
        token.clear();
    }

    while i < n {
        let b = bytes[i];

        if in_string {
            out.push(b as char);
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
                // Fin de chaîne : mettre à jour l'état du conteneur.
                if let Some(top) = stack.last_mut() {
                    match top {
                        Frame::Obj(ObjState::KeyOrClose) => *top = Frame::Obj(ObjState::Colon),
                        Frame::Obj(ObjState::Value) => *top = Frame::Obj(ObjState::CommaOrClose),
                        Frame::Arr(ArrState::ValueOrClose) => *top = Frame::Arr(ArrState::CommaOrClose),
                        _ => {}
                    }
                }
            }
            i += 1;
            continue;
        }

        match b {
            b'"' => {
                // Début de chaîne : flush token précédent, virgule différée.
                flush_token(&mut token, &mut out);
                mark_value_done(&mut stack);
                emit_pending_comma(&mut pending_comma, &mut out);
                out.push('"');
                in_string = true;
                escape = false;
                i += 1;
            }
            b'{' => {
                flush_token(&mut token, &mut out);
                mark_value_done(&mut stack);
                emit_pending_comma(&mut pending_comma, &mut out);
                out.push('{');
                stack.push(Frame::Obj(ObjState::KeyOrClose));
                i += 1;
            }
            b'[' => {
                flush_token(&mut token, &mut out);
                mark_value_done(&mut stack);
                emit_pending_comma(&mut pending_comma, &mut out);
                out.push('[');
                stack.push(Frame::Arr(ArrState::ValueOrClose));
                i += 1;
            }
            b'}' => {
                flush_token(&mut token, &mut out);
                // Virgule terminale éventuelle : abandonnée.
                pending_comma = false;
                // Fermer les conteneurs intermédiaires jusqu'à trouver un objet.
                loop {
                    match stack.last() {
                        Some(Frame::Obj(_)) => {
                            stack.pop();
                            out.push('}');
                            mark_value_done(&mut stack);
                            break;
                        }
                        Some(Frame::Arr(_)) => {
                            stack.pop();
                            out.push(']');
                        }
                        None => break,
                    }
                }
                i += 1;
            }
            b']' => {
                flush_token(&mut token, &mut out);
                // Virgule terminale éventuelle : abandonnée.
                pending_comma = false;
                // Fermer les conteneurs intermédiaires jusqu'à trouver un tableau.
                loop {
                    match stack.last() {
                        Some(Frame::Arr(_)) => {
                            stack.pop();
                            out.push(']');
                            mark_value_done(&mut stack);
                            break;
                        }
                        Some(Frame::Obj(_)) => {
                            stack.pop();
                            out.push('}');
                        }
                        None => break,
                    }
                }
                i += 1;
            }
            b':' => {
                flush_token(&mut token, &mut out);
                out.push(':');
                if let Some(top) = stack.last_mut() {
                    if let Frame::Obj(ObjState::Colon) = top {
                        *top = Frame::Obj(ObjState::Value);
                    }
                }
                i += 1;
            }
            b',' => {
                flush_token(&mut token, &mut out);
                // La valeur scalaire qui précédait la virgule est consommée.
                mark_value_done(&mut stack);
                // Virgule différée : on ne l'émet pas tout de suite.
                pending_comma = true;
                if let Some(top) = stack.last_mut() {
                    match top {
                        Frame::Obj(ObjState::CommaOrClose) => *top = Frame::Obj(ObjState::KeyOrClose),
                        Frame::Arr(ArrState::CommaOrClose) => *top = Frame::Arr(ArrState::ValueOrClose),
                        _ => {}
                    }
                }
                i += 1;
            }
            b' ' | b'\t' | b'\n' | b'\r' => {
                // Espace : flush token, matérialiser la virgule différée AVANT
                // l'espace (pour ne pas produire ` ,`), puis conserver l'espace.
                flush_token(&mut token, &mut out);
                emit_pending_comma(&mut pending_comma, &mut out);
                out.push(b as char);
                i += 1;
            }
            _ => {
                // Octet de token scalaire (chiffre, lettre, signe).
                token.push(b as char);
                i += 1;
            }
        }
    }

    // Fin d'entrée : flush token, fermer chaîne ouverte, compléter membres.
    if !token.is_empty() {
        // Une valeur scalaire est en cours : matérialiser la virgule différée
        // avant elle, puis la marquer consommée.
        emit_pending_comma(&mut pending_comma, &mut out);
        flush_token(&mut token, &mut out);
        mark_value_done(&mut stack);
    }
    if in_string {
        // Backslash pendant supprimé, puis fermeture de chaîne.
        if escape {
            out.pop();
        }
        out.push('"');
        if let Some(top) = stack.last_mut() {
            match top {
                Frame::Obj(ObjState::Value) => *top = Frame::Obj(ObjState::CommaOrClose),
                Frame::Arr(ArrState::ValueOrClose) => *top = Frame::Arr(ArrState::CommaOrClose),
                _ => {}
            }
        }
    }

    // Compléter les membres d'objet incomplets et fermer LIFO.
    while let Some(frame) = stack.pop() {
        match frame {
            Frame::Obj(ObjState::Colon) => {
                // `{"a"` → `:null`
                out.push_str(":null");
                out.push('}');
            }
            Frame::Obj(ObjState::Value) => {
                // `{"a":` → `null`
                out.push_str("null");
                out.push('}');
            }
            Frame::Obj(_) => out.push('}'),
            Frame::Arr(_) => out.push(']'),
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_lifo_order() {
        assert_eq!(repair_structural(r#"{"a": [1, 2"#), r#"{"a": [1, 2]}"#);
        assert_eq!(repair_structural(r#"{"a": {"b": 1"#), r#"{"a": {"b": 1}}"#);
        assert_eq!(repair_structural("[1, [2, [3"), "[1, [2, [3]]]");
    }

    #[test]
    fn nesting_close_order_mixed() {
        assert_eq!(repair_structural(r#"{"a": [1, {"b": 2"#), r#"{"a": [1, {"b": 2}]}"#);
    }

    #[test]
    fn trailing_comma_removed() {
        assert_eq!(repair_structural(r#"{"a": 1,"#), r#"{"a": 1}"#);
        assert_eq!(repair_structural("[1, 2,"), "[1, 2]");
    }

    #[test]
    fn truncated_member_object_completed() {
        assert_eq!(repair_structural(r#"{"a":"#), r#"{"a":null}"#);
        assert_eq!(repair_structural(r#"{"a""#), r#"{"a":null}"#);
    }

    #[test]
    fn object_completed_after_value() {
        assert_eq!(repair_structural(r#"{"a": 1"#), r#"{"a": 1}"#);
    }

    #[test]
    fn escape_backslash_parity() {
        assert_eq!(repair_structural(r#"{"s": "val\""#), r#"{"s": "val\""}"#);
    }

    #[test]
    fn escape_backslash_parity_even_run() {
        // Deux backslashes consécutifs : le guillemet suivant ferme la chaîne.
        assert_eq!(repair_structural(r#"{"s": "a\\"#), r#"{"s": "a\\"}"#);
    }

    #[test]
    fn unterminated_string_closed() {
        assert_eq!(repair_structural(r#"{"a": "hello"#), r#"{"a": "hello"}"#);
    }

    #[test]
    fn open_string_pending_backslash_removed() {
        assert_eq!(repair_structural(r#"{"a": "x\"#), r#"{"a": "x"}"#);
    }

    #[test]
    fn truncated_literal_completed() {
        assert_eq!(repair_structural(r#"{"a": tru"#), r#"{"a": true}"#);
        assert_eq!(repair_structural(r#"{"a": nu"#), r#"{"a": null}"#);
        assert_eq!(repair_structural(r#"{"a": fal"#), r#"{"a": false}"#);
    }

    #[test]
    fn truncated_number_suffix_removed() {
        assert_eq!(repair_structural(r#"{"a": 3.1e"#), r#"{"a": 3.1}"#);
        assert_eq!(repair_structural(r#"{"a": 42."#), r#"{"a": 42}"#);
    }

    #[test]
    fn number_truncated_kept() {
        // (marqueur de validation C10 — contenu inchangé)
        assert_eq!(repair_structural(r#"{"a": 12"#), r#"{"a": 12}"#);
    }

    #[test]
    fn orphan_closer_removed() {
        assert_eq!(repair_structural("1]"), "1");
        assert_eq!(repair_structural("[]]"), "[]");
    }

    #[test]
    fn deeper_closer_closes_intermediate() {
        assert_eq!(repair_structural(r#"{"a": [1}"#), r#"{"a": [1]}"#);
    }

    #[test]
    fn empty_containers_preserved() {
        assert_eq!(repair_structural("{}"), "{}");
        assert_eq!(repair_structural("[]"), "[]");
        assert_eq!(repair_structural(r#"{"a": {}, "b": []}"#), r#"{"a": {}, "b": []}"#);
    }

    #[test]
    fn nested_arrays_lifo() {
        assert_eq!(repair_structural("[[[[1"), "[[[[1]]]]");
        assert_eq!(repair_structural(r#"{"a": [{"b": [1"#), r#"{"a": [{"b": [1]}]}"#);
    }
}
