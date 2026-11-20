//! Types d'erreur de `tool-guard`.
//!
//! Règle d'or (contrat C9) : chaque erreur doit être **actionnable** —
//! c'est-à-dire indiquer à l'appelant ce qu'il doit corriger (chemin invalide,
//! taille excessive, valeur hors bornes, alias inconnu...).
//!
//! L'enum canonique est [`GuardError`]. Les modules `file` et `params` l'utilisent
//! directement ; le module `path` utilise l'alias [`ToolGuardError`].

/// Erreurs de validation défensive de `tool-guard`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardError {
    /// Le chemin fourni est vide ou illisible.
    EmptyPath,
    /// Le chemin tente un parcours (`..`) hors de la racine autorisée.
    PathTraversal { requested: String, allowed_root: String },
    /// Le chemin pointe vers une zone système interdite.
    SystemPath { requested: String },
    /// Le fichier fourni est binaire (non-texte).
    BinaryFile { path: String },
    /// La taille du fichier dépasse la limite autorisée.
    FileTooLarge { size: u64, max: u64 },
    /// L'alias de chemin n'est pas résolu dans la configuration.
    UnknownAlias { alias: String },
    /// Une valeur numérique entière est hors bornes.
    OutOfBounds { field: String, value: i64, min: i64, max: i64 },
    /// Un paramètre numérique est hors de sa plage déclarée.
    OutOfRange { name: String, value: i64, min: i64, max: i64 },
    /// Le fichier ou la ressource est introuvable.
    NotFound { path: String },
    /// Erreur de configuration de `tool-guard`.
    Config(String),
    /// Valeur invalide (ex : flottant non fini).
    Invalid(String),
}

/// Alias backward-compat utilisé par le module [`crate::path`].
pub type ToolGuardError = GuardError;

impl std::fmt::Display for GuardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GuardError::EmptyPath => write!(
                f,
                "Chemin vide fourni. Action : fournissez un chemin de fichier non vide (ex. \"src/main.rs\")."
            ),
            GuardError::PathTraversal { requested, allowed_root } => write!(
                f,
                "Parcours de chemin refusé pour `{requested}` : il sort de la racine autorisée `{allowed_root}`. \
                 Action : utilisez un chemin relatif restant à l'intérieur de la racine, sans segments `..`."
            ),
            GuardError::SystemPath { requested } => write!(
                f,
                "Chemin système interdit : `{requested}`. \
                 Action : limitez-vous aux fichiers du projet ; les chemins système (ex. /etc, C:\\Windows, /proc) ne sont pas autorisés."
            ),
            GuardError::BinaryFile { path } => write!(
                f,
                "Fichier binaire détecté : `{path}`. \
                 Action : fournissez un fichier texte (source, JSON, Markdown) ou convertissez-le avant lecture."
            ),
            GuardError::FileTooLarge { size, max } => write!(
                f,
                "Fichier trop volumineux : {size} octets, la limite est de {max} octets. \
                 Action : réduisez la taille ou ciblez un extrait plus petit."
            ),
            GuardError::UnknownAlias { alias } => write!(
                f,
                "Alias inconnu : `{alias}`. \
                 Action : utilisez un alias déclaré dans la configuration ou le chemin relatif du projet."
            ),
            GuardError::OutOfBounds { field, value, min, max } => write!(
                f,
                "Valeur hors bornes pour `{field}` : {value} (intervalle attendu [{min}, {max}]). \
                 Action : fournissez une valeur comprise entre {min} et {max}."
            ),
            GuardError::OutOfRange { name, value, min, max } => write!(
                f,
                "Paramètre `{name}` hors plage : {value} (intervalle attendu [{min}, {max}]). \
                 Action : fournissez une valeur comprise entre {min} et {max}."
            ),
            GuardError::NotFound { path } => write!(
                f,
                "Ressource introuvable : `{path}`. \
                 Action : vérifiez l'orthographe et l'existence du chemin dans le workspace."
            ),
            GuardError::Config(detail) => write!(
                f,
                "Configuration invalide : {detail}. Action : corrigez la configuration de tool-guard avant de réessayer."
            ),
            GuardError::Invalid(detail) => write!(
                f,
                "Valeur invalide : {detail}. Action : fournissez une valeur valide et finie."
            ),
        }
    }
}

impl std::error::Error for GuardError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contrat C9 : tout message d'erreur doit rester actionnable (mentionne une
    /// action à mener) et être non vide, stable et déterministe.
    #[test]
    fn test_error_display_actionable() {
        let cases: Vec<GuardError> = vec![
            GuardError::EmptyPath,
            GuardError::PathTraversal {
                requested: "../../etc/passwd".into(),
                allowed_root: "D:/Projet/tool-guard".into(),
            },
            GuardError::SystemPath {
                requested: "C:\\Windows".into(),
            },
            GuardError::BinaryFile {
                path: "assets/logo.png".into(),
            },
            GuardError::FileTooLarge { size: 200, max: 100 },
            GuardError::UnknownAlias { alias: "proj".into() },
            GuardError::OutOfBounds {
                field: "start_line".into(),
                value: -1,
                min: 0,
                max: 100,
            },
            GuardError::OutOfRange {
                name: "n".into(),
                value: 11,
                min: 0,
                max: 10,
            },
            GuardError::NotFound { path: "ghost.rs".into() },
            GuardError::Config("borne min > max".into()),
            GuardError::Invalid("NaN".into()),
        ];

        for err in cases {
            let msg = err.to_string();
            assert!(!msg.is_empty(), "le message d'erreur ne doit jamais être vide");
            assert!(
                msg.contains("Action") || msg.contains("action"),
                "le message d'erreur doit être actionnable : {msg}"
            );
        }
    }
}
