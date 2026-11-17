//! Matching contextuel hiérarchique : `exact` > `fuzzy` > `not_found`.
//!
//! Le matcher localise un bloc `target` dans un document `source`. En cas
//! d'échec, il expose un contexte (`context`) permettant au caller de
//! diagnostiquer la divergence.

/// Statut du matching hiérarchique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchStatus {
    /// Correspondance exacte (priorité maximale).
    Exact,
    /// Cible trouvée plusieurs fois : position non unique, le caller doit trancher.
    Ambiguous,
    /// Correspondance approchée (au-delà du seuil de similarité).
    Fuzzy,
    /// Aucune correspondance : le contexte est fourni au caller.
    NotFound,
}

/// Position 1-based dans un document (ligne / colonne).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

/// Résultat d'un appel à [`match_block`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchResult {
    /// Statut hiérarchique du match.
    pub status: MatchStatus,
    /// Position de début du match (si trouvé).
    pub position: Option<Position>,
    /// Contexte de diagnostic (fourni sur `NotFound`, ou sur ambiguïté).
    pub context: Option<String>,
}

/// Seuil de similarité (0.0 à 1.0) pour accepter un match `fuzzy`.
const FUZZY_THRESHOLD: f64 = 0.8;

/// Calcule un taux de similarité par lignes communes / max(lignes).
fn similarity(a: &str, b: &str) -> f64 {
    let la = a.lines().count();
    let lb = b.lines().count();
    let denom = la.max(lb);
    if denom == 0 {
        return 1.0;
    }
    // similarité simple : ratio de longueur + partage de prefixe de lignes
    let common = a.lines().zip(b.lines()).take_while(|(x, y)| x == y).count();
    (common as f64) / (denom as f64)
}

/// Localise `target` dans `source` selon la hiérarchie
/// `exact` → `fuzzy` → `not_found`.
pub fn match_block(source: &str, target: &str) -> MatchResult {
    if target.trim().is_empty() {
        return MatchResult {
            status: MatchStatus::NotFound,
            position: None,
            context: Some("cible vide".to_string()),
        };
    }

    // 1) Exact (priorité max) : sous-chaine littérale.
    if source.contains(target) {
        let occurrences = source.matches(target).count();
        if occurrences > 1 {
            return MatchResult {
                status: MatchStatus::Ambiguous,
                position: None,
                context: Some(format!(
                    "cible présente {} fois — spécifiez un contexte plus large",
                    occurrences
                )),
            };
        }
        // occurrence unique → Exact
        let byte_idx = source.find(target).unwrap();
        let before = &source[..byte_idx];
        let line = before.lines().count() + 1;
        let column = before.rfind('\n').map(|i| byte_idx - i).unwrap_or(byte_idx) + 1;
        return MatchResult {
            status: MatchStatus::Exact,
            position: Some(Position { line, column }),
            context: None,
        };
    }

    // 2) Fuzzy : ligne par ligne au-delà du seuil.
    for (idx, line) in source.lines().enumerate() {
        for (tidx, tline) in target.lines().enumerate() {
            if !tline.trim().is_empty() && similarity(line, tline) >= FUZZY_THRESHOLD {
                return MatchResult {
                    status: MatchStatus::Fuzzy,
                    position: Some(Position { line: idx + 1, column: 1 }),
                    context: Some(format!("similarité {:.2} (ligne {})", FUZZY_THRESHOLD, tidx + 1)),
                };
            }
        }
    }

    // 3) Not found : contexte fourni au caller.
    MatchResult {
        status: MatchStatus::NotFound,
        position: None,
        context: Some(format!("aucun match dans {} lignes", source.lines().count())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_wins() {
        let r = match_block("fn main() {\n    let x = 1;\n}", "let x = 1;");
        assert_eq!(r.status, MatchStatus::Exact);
        assert!(r.position.is_some());
    }

    #[test]
    fn not_found_provides_context() {
        let r = match_block("a\nb\nc", "zzz-not-here");
        assert_eq!(r.status, MatchStatus::NotFound);
        assert!(r.context.is_some());
    }
}