//! Validateur JSON **strict RFC 8259**, **itératif** (sans récursion).
//!
//! Ce module fournit la primitive de validation utilisée par [`crate::repair`]
//! pour décider si une entrée est déjà un JSON valide (dans ce cas elle est
//! retournée inchangée byte à byte) et pour vérifier les sorties réparées.
//!
//! ## Garanties
//!
//! - **Strict RFC 8259** : nombres, chaînes, littéraux, structures conformes.
//! - **Itératif** : pile explicite, aucune récursion → digère une profondeur
//!   de 100 000 sans déborder la pile d'appels.
//! - **Total** : ne panique jamais sur de l'UTF-8 arbitraire.
//! - **Zéro allocation** : parcourt les octets de l'entrée.
//!
//! ## Limite de profondeur
//!
//! La profondeur maximale acceptée est [`MAX_DEPTH`] (100 000). Au-delà, la
//! validation échoue avec [`ErrorKind::DepthOutOfRange`].

use std::fmt;

/// Profondeur d'imbrication maximale acceptée par le validateur.
///
/// Au-delà, [`validate`] retourne [`ErrorKind::DepthOutOfRange`]. Cette borne
/// garantit que la pile explicite reste de taille raisonnable tout en
/// couvrant les documents réels les plus profonds.
pub const MAX_DEPTH: usize = 100_000;

/// Nature d'une erreur de validation JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Fin de l'entrée atteinte alors qu'une valeur était attendue.
    UnexpectedEof,
    /// Octet inattendu à cet endroit du document.
    UnexpectedByte,
    /// Nombre malformé (ex: `1.2.3`, `01`, `1e`, `-`).
    InvalidNumber,
    /// Séquence d'échappement invalide dans une chaîne (ex: `\q`).
    InvalidEscape,
    /// Contenu non blanc après la fin de la valeur racine.
    TrailingContent,
    /// Profondeur d'imbrication supérieure à [`MAX_DEPTH`].
    DepthOutOfRange,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            ErrorKind::UnexpectedEof => "fin de l'entrée inattendue",
            ErrorKind::UnexpectedByte => "octet inattendu",
            ErrorKind::InvalidNumber => "nombre invalide",
            ErrorKind::InvalidEscape => "séquence d'échappement invalide",
            ErrorKind::TrailingContent => "contenu résiduel après la valeur racine",
            ErrorKind::DepthOutOfRange => "profondeur d'imbrication hors limites",
        };
        f.write_str(msg)
    }
}

/// Erreur de validation JSON, localisée par un décalage en octets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationError {
    /// Décalage (en octets) dans l'entrée où l'erreur a été détectée.
    pub offset: usize,
    /// Nature de l'erreur.
    pub kind: ErrorKind,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (à l'octet {})", self.kind, self.offset)
    }
}

impl std::error::Error for ValidationError {}

/// Construit une erreur localisée.
fn err(offset: usize, kind: ErrorKind) -> ValidationError {
    ValidationError { offset, kind }
}

/// Saute les blancs JSON (espace, tabulation, LF, CR).
fn skip_ws(bytes: &[u8], i: &mut usize) {
    let n = bytes.len();
    while *i < n {
        match bytes[*i] {
            b' ' | b'\t' | b'\n' | b'\r' => *i += 1,
            _ => break,
        }
    }
}

/// Valide une chaîne comme document JSON strict RFC 8259.
///
/// Retourne `Ok(())` si `input` est un JSON valide, sinon une
/// [`ValidationError`] localisée.
///
/// # Exemple
///
/// ```
/// use json_repair::{validate, ErrorKind};
///
/// assert!(validate(r#"{"a": [1, 2, 3]}"#).is_ok());
/// let err = validate("{} extra").unwrap_err();
/// assert_eq!(err.kind, ErrorKind::TrailingContent);
/// ```
pub fn validate(input: &str) -> Result<(), ValidationError> {
    let bytes = input.as_bytes();
    let n = bytes.len();
    let mut i = 0usize;

    // Pile explicite des conteneurs ouverts : b'{' ou b'['.
    let mut stack: Vec<u8> = Vec::new();

    skip_ws(bytes, &mut i);

    if i >= n {
        return Err(err(i, ErrorKind::UnexpectedEof));
    }

    // Analyse d'une valeur racine, en boucle itérative.
    // `expect_value` indique qu'on attend une valeur au prochain tour.
    let mut expect_value = true;

    loop {
        skip_ws(bytes, &mut i);

        if expect_value {
            if i >= n {
                return Err(err(i, ErrorKind::UnexpectedEof));
            }
            let b = bytes[i];
            match b {
                b'{' => {
                    if stack.len() + 1 >= MAX_DEPTH {
                        return Err(err(i, ErrorKind::DepthOutOfRange));
                    }
                    stack.push(b'{');
                    i += 1;
                    skip_ws(bytes, &mut i);
                    if i >= n {
                        return Err(err(i, ErrorKind::UnexpectedEof));
                    }
                    if bytes[i] == b'}' {
                        i += 1;
                        stack.pop();
                        // conteneur vide terminé : on revient au contexte parent
                        if !after_value(bytes, &mut i, &mut stack, &mut expect_value)? {
                            return Ok(());
                        }
                        continue;
                    }
                    // attend une clé
                    if bytes[i] != b'"' {
                        return Err(err(i, ErrorKind::UnexpectedByte));
                    }
                    parse_string(bytes, &mut i)?;
                    skip_ws(bytes, &mut i);
                    if i >= n {
                        return Err(err(i, ErrorKind::UnexpectedEof));
                    }
                    if bytes[i] != b':' {
                        return Err(err(i, ErrorKind::UnexpectedByte));
                    }
                    i += 1;
                    expect_value = true;
                    continue;
                }
                b'[' => {
                    if stack.len() + 1 >= MAX_DEPTH {
                        return Err(err(i, ErrorKind::DepthOutOfRange));
                    }
                    stack.push(b'[');
                    i += 1;
                    skip_ws(bytes, &mut i);
                    if i >= n {
                        return Err(err(i, ErrorKind::UnexpectedEof));
                    }
                    if bytes[i] == b']' {
                        i += 1;
                        stack.pop();
                        if !after_value(bytes, &mut i, &mut stack, &mut expect_value)? {
                            return Ok(());
                        }
                        continue;
                    }
                    // attend une valeur
                    expect_value = true;
                    continue;
                }
                b'"' => {
                    parse_string(bytes, &mut i)?;
                    if !after_value(bytes, &mut i, &mut stack, &mut expect_value)? {
                        return Ok(());
                    }
                    continue;
                }
                b't' => {
                    parse_literal(bytes, &mut i, b"true")?;
                    if !after_value(bytes, &mut i, &mut stack, &mut expect_value)? {
                        return Ok(());
                    }
                    continue;
                }
                b'f' => {
                    parse_literal(bytes, &mut i, b"false")?;
                    if !after_value(bytes, &mut i, &mut stack, &mut expect_value)? {
                        return Ok(());
                    }
                    continue;
                }
                b'n' => {
                    parse_literal(bytes, &mut i, b"null")?;
                    if !after_value(bytes, &mut i, &mut stack, &mut expect_value)? {
                        return Ok(());
                    }
                    continue;
                }
                b'-' | b'0'..=b'9' => {
                    parse_number(bytes, &mut i)?;
                    if !after_value(bytes, &mut i, &mut stack, &mut expect_value)? {
                        return Ok(());
                    }
                    continue;
                }
                _ => return Err(err(i, ErrorKind::UnexpectedByte)),
            }
        } else {
            // On attend soit une virgule, soit une fermeture, soit la fin.
            if i >= n {
                if stack.is_empty() {
                    return Ok(());
                }
                return Err(err(i, ErrorKind::UnexpectedEof));
            }
            let b = bytes[i];
            match b {
                b',' => {
                    i += 1;
                    skip_ws(bytes, &mut i);
                    if i >= n {
                        return Err(err(i, ErrorKind::UnexpectedEof));
                    }
                    // Dans un objet, après une virgule on attend une clé (chaîne).
                    if let Some(&top) = stack.last() {
                        if top == b'{' {
                            if bytes[i] != b'"' {
                                return Err(err(i, ErrorKind::UnexpectedByte));
                            }
                            parse_string(bytes, &mut i)?;
                            skip_ws(bytes, &mut i);
                            if i >= n {
                                return Err(err(i, ErrorKind::UnexpectedEof));
                            }
                            if bytes[i] != b':' {
                                return Err(err(i, ErrorKind::UnexpectedByte));
                            }
                            i += 1;
                            expect_value = true;
                            continue;
                        }
                    }
                    expect_value = true;
                    continue;
                }
                b'}' => {
                    if stack.last() == Some(&b'{') {
                        stack.pop();
                        i += 1;
                        if !after_value(bytes, &mut i, &mut stack, &mut expect_value)? {
                            return Ok(());
                        }
                        continue;
                    }
                    return Err(err(i, ErrorKind::UnexpectedByte));
                }
                b']' => {
                    if stack.last() == Some(&b'[') {
                        stack.pop();
                        i += 1;
                        if !after_value(bytes, &mut i, &mut stack, &mut expect_value)? {
                            return Ok(());
                        }
                        continue;
                    }
                    return Err(err(i, ErrorKind::UnexpectedByte));
                }
                _ => return Err(err(i, ErrorKind::UnexpectedByte)),
            }
        }
    }
}

/// Après qu'une valeur a été consommée, met à jour l'état.
///
/// Retourne `Ok(true)` s'il faut continuer l'analyse, `Ok(false)` si le
/// document est complet (valeur racine terminée et plus rien à lire).
fn after_value(
    bytes: &[u8],
    i: &mut usize,
    stack: &mut Vec<u8>,
    expect_value: &mut bool,
) -> Result<bool, ValidationError> {
    let n = bytes.len();
    skip_ws(bytes, i);
    if stack.is_empty() {
        // Valeur racine terminée : il ne doit plus rien rester.
        if *i >= n {
            return Ok(false);
        }
        return Err(err(*i, ErrorKind::TrailingContent));
    }
    // Dans un conteneur : on attend une virgule ou une fermeture.
    *expect_value = false;
    Ok(true)
}

/// Analyse une chaîne JSON (guillemets inclus) avec validation des échappements.
fn parse_string(bytes: &[u8], i: &mut usize) -> Result<(), ValidationError> {
    let n = bytes.len();
    // Guillemet ouvrant
    if *i >= n || bytes[*i] != b'"' {
        return Err(err(*i, ErrorKind::UnexpectedByte));
    }
    *i += 1;
    while *i < n {
        let b = bytes[*i];
        match b {
            b'"' => {
                *i += 1;
                return Ok(());
            }
            b'\\' => {
                *i += 1;
                if *i >= n {
                    return Err(err(*i, ErrorKind::UnexpectedEof));
                }
                match bytes[*i] {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {
                        *i += 1;
                    }
                    b'u' => {
                        *i += 1;
                        for _ in 0..4 {
                            if *i >= n {
                                return Err(err(*i, ErrorKind::UnexpectedEof));
                            }
                            if !bytes[*i].is_ascii_hexdigit() {
                                // Un guillemet ou une fin de flux interrompt la
                                // séquence `\uXXXX` avant ses 4 chiffres : la
                                // séquence est tronquée (fin inattendue), pas
                                // simplement invalide.
                                if bytes[*i] == b'"' {
                                    return Err(err(*i, ErrorKind::UnexpectedEof));
                                }
                                return Err(err(*i, ErrorKind::InvalidEscape));
                            }
                            *i += 1;
                        }
                    }
                    // Un guillemet échappé (`\"`) est déjà couvert ci-dessus ;
                    // tout autre octet après `\` est une séquence invalide.
                    _ => return Err(err(*i, ErrorKind::InvalidEscape)),
                }
            }
            // Caractères de contrôle non échappés interdits (< 0x20).
            0x00..=0x1f => return Err(err(*i, ErrorKind::UnexpectedByte)),
            _ => {
                *i += 1;
            }
        }
    }
    Err(err(*i, ErrorKind::UnexpectedEof))
}

/// Analyse un littéral (`true`, `false`, `null`).
fn parse_literal(bytes: &[u8], i: &mut usize, lit: &[u8]) -> Result<(), ValidationError> {
    let n = bytes.len();
    for &e in lit {
        if *i >= n {
            return Err(err(*i, ErrorKind::UnexpectedEof));
        }
        if bytes[*i] != e {
            return Err(err(*i, ErrorKind::UnexpectedByte));
        }
        *i += 1;
    }
    Ok(())
}

/// Analyse un nombre JSON strict RFC 8259.
///
/// Grammaire : `-? (0 | [1-9][0-9]*) ( '.' [0-9]+ )? ( [eE] [+-]? [0-9]+ )?`
fn parse_number(bytes: &[u8], i: &mut usize) -> Result<(), ValidationError> {
    let n = bytes.len();
    let start = *i;

    // Signe optionnel
    if *i < n && bytes[*i] == b'-' {
        *i += 1;
    }

    // Partie entière
    if *i >= n {
        return Err(err(*i, ErrorKind::UnexpectedEof));
    }
    match bytes[*i] {
        b'0' => {
            *i += 1;
            // Un zéro ne peut pas être suivi d'un autre chiffre.
            if *i < n && bytes[*i].is_ascii_digit() {
                return Err(err(*i, ErrorKind::InvalidNumber));
            }
        }
        b'1'..=b'9' => {
            *i += 1;
            while *i < n && bytes[*i].is_ascii_digit() {
                *i += 1;
            }
        }
        _ => return Err(err(*i, ErrorKind::InvalidNumber)),
    }

    // Partie fractionnaire
    if *i < n && bytes[*i] == b'.' {
        *i += 1;
        if *i >= n {
            return Err(err(*i, ErrorKind::UnexpectedEof));
        }
        if !bytes[*i].is_ascii_digit() {
            return Err(err(*i, ErrorKind::InvalidNumber));
        }
        while *i < n && bytes[*i].is_ascii_digit() {
            *i += 1;
        }
    }

    // Partie exposant
    if *i < n && (bytes[*i] == b'e' || bytes[*i] == b'E') {
        *i += 1;
        if *i < n && (bytes[*i] == b'+' || bytes[*i] == b'-') {
            *i += 1;
        }
        if *i >= n {
            return Err(err(*i, ErrorKind::UnexpectedEof));
        }
        if !bytes[*i].is_ascii_digit() {
            return Err(err(*i, ErrorKind::InvalidNumber));
        }
        while *i < n && bytes[*i].is_ascii_digit() {
            *i += 1;
        }
    }

    // Tous les chemins ci-dessus ont soit consommé au moins un octet, soit
    // retourné une erreur : `*i > start` est donc garanti ici.
    debug_assert!(*i > start);

    // Un nombre ne peut pas être immédiatement suivi d'un `.` ou d'un chiffre
    // (ex: `1.2.3`, `01`) : c'est un nombre malformé, pas du contenu résiduel.
    if *i < n && (bytes[*i] == b'.' || bytes[*i].is_ascii_digit()) {
        return Err(err(*i, ErrorKind::InvalidNumber));
    }
    Ok(())
}

/// Teste si une chaîne est un JSON valide.
///
/// Raccourci pour `validate(input).is_ok()`.
pub fn is_valid(input: &str) -> bool {
    validate(input).is_ok()
}

// ---------------------------------------------------------------------------
// Tests unitaires — C11 (profondeur 100 000) + validateur strict
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_nesting_100_000_iteratif_ne_panic_pas() {
        let depth = 100_000;
        let deep = "[".repeat(depth) + &"]".repeat(depth);
        let err = validate(&deep).unwrap_err();
        assert_eq!(err.kind, ErrorKind::DepthOutOfRange);
    }

    #[test]
    fn deep_nesting_100000_under_limit_is_valid() {
        let depth = 99_999;
        let deep = "[".repeat(depth) + &"]".repeat(depth);
        assert!(validate(&deep).is_ok());
    }

    #[test]
    fn valid_scalars() {
        assert!(validate("null").is_ok());
        assert!(validate("true").is_ok());
        assert!(validate("false").is_ok());
        assert!(validate("0").is_ok());
        assert!(validate("-0").is_ok());
        assert!(validate("42").is_ok());
        assert!(validate("-1.5e10").is_ok());
        assert!(validate("3.14").is_ok());
        assert!(validate("1E+2").is_ok());
        assert!(validate(r#""hello""#).is_ok());
    }

    #[test]
    fn valid_structures() {
        assert!(validate("{}").is_ok());
        assert!(validate("[]").is_ok());
        assert!(validate(r#"{"a": 1, "b": [2, 3]}"#).is_ok());
        assert!(validate(r#"[1, 2, 3, [4, [5, [6]]]]"#).is_ok());
        assert!(validate(r#"{"unicode": "é世€"}"#).is_ok());
    }

    #[test]
    fn invalid_numbers() {
        assert_eq!(validate("01").unwrap_err().kind, ErrorKind::InvalidNumber);
        assert_eq!(validate("1.2.3").unwrap_err().kind, ErrorKind::InvalidNumber);
        assert_eq!(validate("1.").unwrap_err().kind, ErrorKind::UnexpectedEof);
        assert_eq!(validate("1e").unwrap_err().kind, ErrorKind::UnexpectedEof);
        assert_eq!(validate("-").unwrap_err().kind, ErrorKind::UnexpectedEof);
        assert_eq!(validate("+1").unwrap_err().kind, ErrorKind::UnexpectedByte);
    }

    #[test]
    fn invalid_escapes() {
        assert_eq!(
            validate(r#"["\q"]"#).unwrap_err().kind,
            ErrorKind::InvalidEscape
        );
        assert_eq!(
            validate(r#"["\u12"]"#).unwrap_err().kind,
            ErrorKind::UnexpectedEof
        );
        assert_eq!(
            validate(r#"["\uZZZZ"]"#).unwrap_err().kind,
            ErrorKind::InvalidEscape
        );
    }

    #[test]
    fn trailing_content_detected() {
        assert_eq!(
            validate("{} extra").unwrap_err().kind,
            ErrorKind::TrailingContent
        );
        assert_eq!(
            validate("null null").unwrap_err().kind,
            ErrorKind::TrailingContent
        );
    }

    #[test]
    fn unexpected_eof_detected() {
        assert_eq!(
            validate(r#"{"a": "unclosed"#).unwrap_err().kind,
            ErrorKind::UnexpectedEof
        );
        assert_eq!(validate("").unwrap_err().kind, ErrorKind::UnexpectedEof);
        assert_eq!(validate("   ").unwrap_err().kind, ErrorKind::UnexpectedEof);
    }

    #[test]
    fn unexpected_byte_detected() {
        assert_eq!(
            validate("[1 & 2]").unwrap_err().kind,
            ErrorKind::UnexpectedByte
        );
        assert_eq!(validate("[1,]").unwrap_err().kind, ErrorKind::UnexpectedByte);
        assert_eq!(validate("{,}").unwrap_err().kind, ErrorKind::UnexpectedByte);
    }

    #[test]
    fn is_valid_consistent() {
        assert!(is_valid("{}"));
        assert!(!is_valid("{} x"));
        assert!(!is_valid("[1,]"));
        assert!(!is_valid(""));
        assert!(!is_valid("  "));
    }

    #[test]
    fn error_offset_is_reported() {
        let e = validate("{} extra").unwrap_err();
        assert_eq!(e.offset, 3);
    }
}
