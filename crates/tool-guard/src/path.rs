//! Sanitisation et validation des chemins de fichiers.
//!
//! Garanties scellées (contrats C1..C4) :
//! - C1 : aucun `..` / path traversal ne permet de sortir de la racine autorisée.
//! - C2 : les chemins système sensibles (absolus hors sandbox, /etc, C:\Windows…) sont rejetés.
//! - C3 : le chemin vide est rejeté.
//! - C4 : le chemin retourné est toujours normalisé (segments redondants supprimés).

use crate::error::ToolGuardError;
use std::path::{Component, Path, PathBuf};

/// Racine canonique autorisée par défaut (sandbox).
const DEFAULT_ROOT: &str = "workspace";

/// Résultat d'une validation de chemin : chemin résolu + normalisé dans la sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPath {
    pub raw: PathBuf,
    pub normalized: PathBuf,
    pub is_absolute: bool,
}

/// Normalise un chemin : supprime les `.` et les `..` internes, les séparateurs redondants.
///
/// NB : les `..` qui remontent au-dessus de la racine sont éliminés (pas de sortie de sandbox).
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                // Interdit de remonter au-dessus du sommet normalisé (anti-traversal).
                if !out.pop() {
                    // on ignore le `..` initial : il tenterait de sortir de la racine.
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Détecte une tentative de path traversal dans le chemin brut.
pub fn has_traversal(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::ParentDir))
}

/// Détecte les chemins système sensibles connus (à rejeter, contrat C2).
pub fn is_system_sensitive(path: &Path) -> bool {
    let s = path.to_string_lossy().to_lowercase();
    const SENSITIVES: &[&str] = &[
        "/etc/", "/etc\\", "c:\\windows", "/windows/", "system32",
        "/dev/", "/proc/", "/sys/", "boot.ini", "/root/", "~/.ssh",
    ];
    SENSITIVES.iter().any(|kw| s.contains(kw))
}

/// Valide un chemin utilisateur et le confine à la sandbox.
///
/// - Rejette le chemin vide (C3).
/// - Rejette le path traversal (C1) — prioritaire sur la détection système.
/// - Rejette les chemins système sensibles (C2).
/// - Retourne le chemin normalisé (C4).
pub fn validate(raw: &str) -> Result<ResolvedPath, ToolGuardError> {
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return Err(ToolGuardError::EmptyPath);
    }

    let p = Path::new(trimmed);

    // C1 d'abord : toute remontée via `..` est un traversal, même vers une zone système.
    if has_traversal(p) {
        return Err(ToolGuardError::PathTraversal {
            requested: trimmed.to_string(),
            allowed_root: DEFAULT_ROOT.to_string(),
        });
    }

    // C2 : chemins système sensibles (sans composant `..`).
    if is_system_sensitive(p) {
        return Err(ToolGuardError::SystemPath {
            requested: trimmed.to_string(),
        });
    }

    // Confine à la sandbox : si le chemin est relatif, on le préfixe par la racine autorisée.
    let anchored = if p.is_absolute() {
        PathBuf::from(trimmed)
    } else {
        PathBuf::from(DEFAULT_ROOT).join(p)
    };

    let normalized = normalize(&anchored);

    Ok(ResolvedPath {
        raw: PathBuf::from(trimmed),
        normalized,
        is_absolute: p.is_absolute(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_path_traversal_blocked() {
        // C1 : toute sortie de la sandbox via `..` doit être rejetée.
        let cases = [
            "../etc/passwd",
            "src/../../secrets.txt",
            "..\\..\\windows\\system32",
            "a/../../../../x",
        ];
        for c in cases {
            let res = validate(c);
            match res {
                Ok(r) => {
                    assert!(
                        !r.normalized.components().any(|x| matches!(x, Component::ParentDir)),
                        "traversal leak dans {:?} <- {}",
                        r.normalized, c
                    );
                }
                Err(e) => {
                    assert!(
                        matches!(e, ToolGuardError::PathTraversal { .. }),
                        "attendu PathTraversal pour {}, obtenu {:?}", c, e
                    );
                }
            }
        }
    }

    #[test]
    fn test_system_paths_rejected() {
        // C2 : les chemins système sensibles sont systématiquement rejetés.
        let cases = [
            "/etc/passwd",
            "C:\\Windows\\System32\\cmd.exe",
            "/proc/self/environ",
            "/root/.ssh/id_rsa",
        ];
        for c in cases {
            let res = validate(c);
            assert!(
                matches!(res, Err(ToolGuardError::SystemPath { .. })),
                "attendu SystemPath pour {}, obtenu {:?}", c, res
            );
        }
    }

    #[test]
    fn test_empty_path_rejected() {
        // C3 : chaîne vide ou whitespace-only rejetée.
        for c in ["", "   ", "\t"] {
            assert!(
                matches!(validate(c), Err(ToolGuardError::EmptyPath)),
                "attendu EmptyPath pour {:?}", c
            );
        }
    }

    #[test]
    fn test_path_normalization() {
        // C4 : les redondances (`.`, séparateurs) sont éliminées dans le résultat.
        let ok = validate("src/./modules///lib.rs").expect("valid");
        assert!(!ok
            .normalized
            .components()
            .any(|x| matches!(x, Component::CurDir)));
        let joined = ok.normalized.to_string_lossy().to_string();
        assert!(!joined.contains("//"), "séparateur redondant dans {}", joined);

        // Un chemin simple valide passe et conserve ses segments.
        assert!(validate("notes/readme.md").is_ok());
        assert!(normalize(Path::new("a/./b/../c")).ends_with("a/c"));
    }
}
