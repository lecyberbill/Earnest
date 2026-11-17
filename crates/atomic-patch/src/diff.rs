//! Génération de diff unifié simple (linéaire, sans LCS).
//!
//! Compare deux contenus ligne à ligne et produit un `Diff` avec des
//! entrées `DiffLine` typées (`Context`, `Added`, `Removed`) ainsi qu'un
//! champ `preview` pour l'aperçu humain.

/// Nature d'une ligne dans un diff unifié.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    /// Ligne identique dans les deux contenus.
    Context,
    /// Ligne présente uniquement dans `new`.
    Added,
    /// Ligne présente uniquement dans `old`.
    Removed,
}

/// Une ligne de diff unifié.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    /// Nature de la ligne.
    pub kind: DiffLineKind,
    /// Contenu de la ligne (sans fin de ligne).
    pub text: String,
    /// Numéro de ligne dans `old` (si applicable), 1-based.
    pub old_line: Option<usize>,
    /// Numéro de ligne dans `new` (si applicable), 1-based.
    pub new_line: Option<usize>,
}

/// Résultat d'une comparaison unifiée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    /// Lignes du diff, ordonnées.
    pub lines: Vec<DiffLine>,
    /// Aperçu humain compact (`+`/`-`/` `).
    pub preview: String,
}

fn prefix(kind: DiffLineKind) -> &'static str {
    match kind {
        DiffLineKind::Context => " ",
        DiffLineKind::Added => "+",
        DiffLineKind::Removed => "-",
    }
}

/// Compare `old` et `new` ligne à ligne (déplacement linéaire).
pub fn generate_unified_diff(old: &str, new: &str) -> Diff {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let mut lines: Vec<DiffLine> = Vec::new();
    let mut old_idx = 0usize;
    let mut new_idx = 0usize;

    while old_idx < old_lines.len() || new_idx < new_lines.len() {
        let o = old_lines.get(old_idx);
        let n = new_lines.get(new_idx);

        match (o, n) {
            (Some(ol), Some(nl)) if ol == nl => {
                lines.push(DiffLine {
                    kind: DiffLineKind::Context,
                    text: ol.to_string(),
                    old_line: Some(old_idx + 1),
                    new_line: Some(new_idx + 1),
                });
                old_idx += 1;
                new_idx += 1;
            }
            (Some(ol), Some(_)) => {
                // Lignes différentes : `old` est retirée.
                lines.push(DiffLine {
                    kind: DiffLineKind::Removed,
                    text: ol.to_string(),
                    old_line: Some(old_idx + 1),
                    new_line: None,
                });
                old_idx += 1;
            }
            (Some(ol), None) => {
                // `new` épuisé, `old` a des lignes restantes : retirées.
                lines.push(DiffLine {
                    kind: DiffLineKind::Removed,
                    text: ol.to_string(),
                    old_line: Some(old_idx + 1),
                    new_line: None,
                });
                old_idx += 1;
            }
            (None, Some(nl)) => {
                // `old` épuisé, `new` a des lignes restantes : ajoutées.
                lines.push(DiffLine {
                    kind: DiffLineKind::Added,
                    text: nl.to_string(),
                    old_line: None,
                    new_line: Some(new_idx + 1),
                });
                new_idx += 1;
            }
            (None, None) => break,
        }
    }

    let preview = lines
        .iter()
        .map(|l| format!("{}{}", prefix(l.kind), l.text))
        .collect::<Vec<_>>()
        .join("\n");

    Diff { lines, preview }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_is_all_context() {
        let d = generate_unified_diff("a\nb", "a\nb");
        assert!(d.lines.iter().all(|l| l.kind == DiffLineKind::Context));
        assert_eq!(d.lines.len(), 2);
    }

    #[test]
    fn preview_accuracy() {
        let d = generate_unified_diff("a", "b");
        assert!(d.lines.iter().any(|l| l.kind == DiffLineKind::Removed));
        assert!(d.lines.iter().any(|l| l.kind == DiffLineKind::Added));
        assert_eq!(d.preview, "-a\n+b");
    }
}