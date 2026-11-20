//! Validation et normalisation des paramètres d'outils.
//!
//! Contrats :
//! - C7 : résolution d'alias de chemin (alias projet -> chemin réel normalisé).
//! - C8 : bornes numériques (rejet des valeurs hors plage / non bornables).

use std::collections::HashMap;

use crate::error::GuardError;

/// Résolout un alias de chemin projet vers un chemin normalisé canonique.
///
/// Les alias connus sont remplacés ; le résultat est normalisé via
/// [`crate::path::normalize`].
pub fn resolve_path_alias(
    alias: &str,
    aliases: &HashMap<String, String>,
) -> Result<String, GuardError> {
    let resolved = aliases.get(alias).cloned().unwrap_or_else(|| alias.to_string());
    let normalized = crate::path::normalize(std::path::Path::new(&resolved));
    Ok(normalized.to_string_lossy().replace('\\', "/"))
}

/// Retourne `v` si elle est dans `min..=max`, sinon une erreur actionnable.
pub fn clamp_numeric(v: i64, min: i64, max: i64, name: &str) -> Result<i64, GuardError> {
    if min > max {
        return Err(GuardError::Config(format!(
            "paramètre {name} : borne min ({min}) > max ({max})"
        )));
    }
    if !(min..=max).contains(&v) {
        return Err(GuardError::OutOfRange {
            name: name.to_string(),
            value: v,
            min,
            max,
        });
    }
    Ok(v)
}

/// Rejette les flottants non finis ou hors plage.
pub fn clamp_f64(v: f64, min: f64, max: f64, name: &str) -> Result<f64, GuardError> {
    if !v.is_finite() {
        return Err(GuardError::Invalid(format!(
            "paramètre {name} : valeur non finie ({v:?})"
        )));
    }
    if !(min..=max).contains(&v) {
        return Err(GuardError::OutOfRange {
            name: name.to_string(),
            value: v as i64,
            min: min as i64,
            max: max as i64,
        });
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_alias_resolution() {
        let mut aliases = HashMap::new();
        aliases.insert("project".to_string(), "D:/Projet/tool-guard".to_string());
        aliases.insert("src".to_string(), "D:/Projet/tool-guard/src".to_string());

        let resolved = resolve_path_alias("src", &aliases).unwrap();
        assert!(resolved.starts_with("D:/Projet"));
        assert!(!resolved.contains(".."));

        // Un chemin inconnu est renvoyé tel quel (mais normalisé).
        let unknown = resolve_path_alias("a/b", &aliases).unwrap();
        assert_eq!(unknown, "a/b");
    }

    #[test]
    fn test_numeric_bounds() {
        assert_eq!(clamp_numeric(5, 0, 10, "n").unwrap(), 5);
        assert_eq!(clamp_numeric(0, 0, 10, "n").unwrap(), 0);
        assert_eq!(clamp_numeric(10, 0, 10, "n").unwrap(), 10);

        assert!(clamp_numeric(-1, 0, 10, "n").is_err());
        assert!(clamp_numeric(11, 0, 10, "n").is_err());
        assert!(clamp_numeric(5, 10, 0, "n").is_err());

        assert_eq!(clamp_f64(1.5, 0.0, 2.0, "f").unwrap(), 1.5);
        assert!(clamp_f64(f64::NAN, 0.0, 2.0, "f").is_err());
        assert!(clamp_f64(f64::INFINITY, 0.0, 2.0, "f").is_err());
    }
}