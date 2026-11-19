//! Génération de diff unifié basée sur un vrai calcul LCS.
//!
//! L'algorithme calcule la plus longue sous-séquence commune (*Longest Common
//! Subsequence*) des lignes de `old` et de `new` par programmation dynamique,
//! puis reconstruit le diff en suivant une trajectoire optimale. Contrairement
//! à un balayage linéaire, une insertion ou une suppression en milieu de
//! fichier ne « décale » plus les lignes suivantes : celles-ci restent en
//! `Context` et [`Diff::stats`] reflète exactement les changements réels.
//!
//! Complexité : O(n × m) en temps et en mémoire (n = lignes de `old`,
//! m = lignes de `new`).
//!
//! Zéro dépendance externe : `std` uniquement.

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

/// Calcule la table LCS aplatie (suffixes) : `table[i * stride + j]` contient
/// la longueur de la plus longue sous-séquence commune de `old[i..]` et
/// `new[j..]`, avec `stride = new.len() + 1`.
fn lcs_table(old: &[&str], new: &[&str]) -> Vec<usize> {
    let stride = new.len() + 1;
    let mut table = vec![0usize; (old.len() + 1) * stride];

    for (i, old_line) in old.iter().enumerate().rev() {
        for (j, new_line) in new.iter().enumerate().rev() {
            let here = i * stride + j;
            table[here] = if old_line == new_line {
                table[(i + 1) * stride + j + 1] + 1
            } else {
                let skip_old = table[(i + 1) * stride + j];
                let skip_new = table[i * stride + j + 1];
                skip_old.max(skip_new)
            };
        }
    }

    table
}

/// Compare `old` et `new` ligne à ligne via un calcul LCS.
///
/// Les lignes communes restent en [`DiffLineKind::Context`] avec leurs numéros
/// d'origine respectifs ; les lignes propres à `old` sont `Removed` (avant les
/// `Added` du même bloc, ordre `-`/`+`), celles propres à `new` sont `Added`.
pub fn generate_unified_diff(old: &str, new: &str) -> Diff {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let old_len = old_lines.len();
    let new_len = new_lines.len();
    let stride = new_len + 1;
    let table = lcs_table(&old_lines, &new_lines);

    let mut lines: Vec<DiffLine> = Vec::with_capacity(old_len + new_len);
    let mut old_idx = 0usize;
    let mut new_idx = 0usize;

    while old_idx < old_len || new_idx < new_len {
        if old_idx < old_len && new_idx < new_len && old_lines[old_idx] == new_lines[new_idx] {
            lines.push(DiffLine {
                kind: DiffLineKind::Context,
                text: old_lines[old_idx].to_string(),
                old_line: Some(old_idx + 1),
                new_line: Some(new_idx + 1),
            });
            old_idx += 1;
            new_idx += 1;
            continue;
        }

        // Suit la branche qui préserve une sous-séquence commune maximale.
        // En cas d'égalité, on retire d'abord (ordre `-` puis `+`).
        let take_removed = if old_idx >= old_len {
            false
        } else if new_idx >= new_len {
            true
        } else {
            table[(old_idx + 1) * stride + new_idx] >= table[old_idx * stride + new_idx + 1]
        };

        if take_removed {
            lines.push(DiffLine {
                kind: DiffLineKind::Removed,
                text: old_lines[old_idx].to_string(),
                old_line: Some(old_idx + 1),
                new_line: None,
            });
            old_idx += 1;
        } else {
            lines.push(DiffLine {
                kind: DiffLineKind::Added,
                text: new_lines[new_idx].to_string(),
                old_line: None,
                new_line: Some(new_idx + 1),
            });
            new_idx += 1;
        }
    }

    let preview = lines
        .iter()
        .map(|l| format!("{}{}", prefix(l.kind), l.text))
        .collect::<Vec<_>>()
        .join("\n");

    Diff { lines, preview }
}

impl Diff {
    /// Retourne `(added_count, removed_count)` en comptant respectivement
    /// les lignes `DiffLineKind::Added` et `DiffLineKind::Removed`.
    pub fn stats(&self) -> (usize, usize) {
        let added = self
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Added)
            .count();
        let removed = self
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Removed)
            .count();
        (added, removed)
    }
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

    #[test]
    fn stats_counts_added_removed() {
        // old="a\nb" → new="a\nc" :
        //   "a"  → Context
        //   "b"  → Removed
        //   "c"  → Added
        let d = generate_unified_diff("a\nb", "a\nc");
        let (added, removed) = d.stats();
        assert_eq!(added, 1, "la ligne 'c' est ajoutée");
        assert_eq!(removed, 1, "la ligne 'b' est retirée");
    }

    #[test]
    fn lcs_mid_file_insert_keeps_suffix_context() {
        // Insertion + remplacement en milieu de fichier : "three" ne doit plus
        // être vu comme Removed+Added (comportement du diff linéaire).
        let d = generate_unified_diff("one\ntwo\nthree", "one\nTWO\nthree\nfour");
        assert_eq!(d.stats(), (2, 1), "TWO + four ajoutées, two retirée");

        let three = d
            .lines
            .iter()
            .find(|l| l.text == "three")
            .expect("la ligne 'three' doit figurer dans le diff");
        assert_eq!(three.kind, DiffLineKind::Context);
        assert_eq!(three.old_line, Some(3));
        assert_eq!(three.new_line, Some(3));
    }

    #[test]
    fn lcs_mid_file_delete_keeps_suffix_context() {
        let d = generate_unified_diff("one\ntwo\nthree\nfour", "one\nthree\nfour");
        assert_eq!(d.stats(), (0, 1));
        assert_eq!(d.lines[0].kind, DiffLineKind::Context);
        assert_eq!(d.lines[0].text, "one");

        let removed: Vec<&str> = d
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Removed)
            .map(|l| l.text.as_str())
            .collect();
        assert_eq!(removed, vec!["two"]);
    }
}
