//! Execution de `node --check` et analyse de sa sortie.

use std::path::Path;
use std::process::Command;

/// Resultat brut d'une execution de `node --check`.
pub struct NodeCheckOutcome {
    /// `true` si la syntaxe est valide.
    pub ok: bool,
    /// Ligne (1-indexee) de l'erreur, si detectable.
    pub error_line: Option<usize>,
    /// Message d'erreur, si echec.
    pub error: Option<String>,
}

/// Execute `node --check <path>` et interprete la sortie.
pub fn run_node_check(path: &Path) -> NodeCheckOutcome {
    let output = match Command::new("node")
        .arg("--check")
        .arg(path)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            return NodeCheckOutcome {
                ok: false,
                error_line: None,
                error: Some(format!("impossible de lancer `node --check` : {e}")),
            }
        }
    };

    if output.status.success() {
        return NodeCheckOutcome {
            ok: true,
            error_line: None,
            error: None,
        };
    }

    let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&stdout);
    }

    NodeCheckOutcome {
        ok: false,
        error_line: parse_error_line(&text),
        error: extract_error_message(&text),
    }
}

/// Extrait le numero de ligne depuis la sortie de `node --check`.
///
/// Node place en debut de sortie l'emplacement de l'erreur sous la forme
/// `<chemin>:<ligne>` ou `<chemin>:<ligne>:<colonne>`.
fn parse_error_line(text: &str) -> Option<usize> {
    for line in text.lines().take(12) {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let parts: Vec<&str> = t.rsplit(':').collect();
        let last = match parts.first() {
            Some(l) => l,
            None => continue,
        };
        let last_is_num = !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit());
        if !last_is_num {
            continue;
        }
        // Forme `<chemin>:<ligne>:<colonne>` : la colonne est le dernier
        // segment numerique, la ligne est le precedent.
        if let Some(prev) = parts.get(1) {
            if !prev.is_empty() && prev.bytes().all(|b| b.is_ascii_digit()) {
                return prev.parse().ok();
            }
        }
        // Forme `<chemin>:<ligne>` : le dernier segment est la ligne.
        return last.parse().ok();
    }
    None
}

/// Extrait le message d'erreur le plus parlant (ex. `SyntaxError: ...`).
fn extract_error_message(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        if t.contains("Error") {
            return Some(t.to_string());
        }
    }
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ligne_simple() {
        let out = "D:/tmp/bad.js:3\nconst a = 1\n\nSyntaxError: Invalid or unexpected token\n";
        assert_eq!(parse_error_line(out), Some(3));
    }

    #[test]
    fn ligne_colonne() {
        let out = "/home/u/bad.js:2:9\nSyntaxError: bad token\n";
        assert_eq!(parse_error_line(out), Some(2));
    }

    #[test]
    fn sans_ligne() {
        assert_eq!(parse_error_line("Erreur inconnue\n"), None);
    }

    #[test]
    fn message_syntaxerror() {
        let out = "bad.js:1\n\nSyntaxError: Invalid or unexpected token\n";
        assert_eq!(
            extract_error_message(out),
            Some("SyntaxError: Invalid or unexpected token".to_string())
        );
    }
}
