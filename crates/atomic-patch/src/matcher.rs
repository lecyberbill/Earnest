//! Matching contextuel hiérarchique : `exact` > `fuzzy` > `not_found`.
//!
//! Le matcher localise un bloc `target` dans un document `source`. En cas
//! d'échec, il expose un contexte (`context`) permettant au caller de
//! diagnostiquer la divergence.
//!
//! [`apply_patch`] va plus loin : il remplace un bloc en tolérant les écarts
//! d'indentation et de fins de ligne (CRLF ↔ LF), sans jamais choisir
//! silencieusement entre deux candidats.

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

impl MatchResult {
    /// `true` si le statut est [`MatchStatus::NotFound`] ou
    /// [`MatchStatus::Ambiguous`] (donc inutilisable sans arbitrage humain).
    pub fn is_error(&self) -> bool {
        matches!(self.status, MatchStatus::NotFound | MatchStatus::Ambiguous)
    }
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

// ---------------------------------------------------------------------------
// Remplacement tolérant (indentation + fins de ligne)
// ---------------------------------------------------------------------------

/// Construit un `MatchResult` d'erreur (aucune position exploitable).
fn error(status: MatchStatus, context: String) -> MatchResult {
    MatchResult {
        status,
        position: None,
        context: Some(context),
    }
}

/// Longueur de l'indentation de tête (espaces et tabulations).
fn indent_len(line: &str) -> usize {
    line.as_bytes()
        .iter()
        .take_while(|b| **b == b' ' || **b == b'\t')
        .count()
}

/// Retire l'indentation commune (*dédent* relatif) d'un bloc de lignes.
///
/// Les lignes vides ou blanches deviennent `""` ; l'indentation relative
/// (sous-blocs imbriqués) est préservée. Le décalage retiré n'est pas
/// retourné : le caller recalcule l'indentation de base du bloc réellement
/// trouvé dans le fichier.
fn dedent_lines<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    let min = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_len(l))
        .min()
        .unwrap_or(0);

    lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                ""
            } else if l.len() >= min {
                &l[min..]
            } else {
                ""
            }
        })
        .collect()
}

/// Localise toutes les fenêtres de `haystack` égales à `needle` après
/// normalisation de l'indentation commune (dédent relatif des deux côtés)
/// et suppression des espaces horizontaux de fin de ligne.
fn fuzzy_matches(haystack: &[&str], needle: &[&str]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }

    let needle_norm = dedent_lines(needle);
    let mut starts = Vec::new();

    for (start, window) in haystack.windows(needle.len()).enumerate() {
        let win_norm = dedent_lines(window);
        let eq_trimmed = win_norm
            .iter()
            .map(|s| s.trim_end_matches([' ', '\t']))
            .eq(needle_norm.iter().map(|s| s.trim_end_matches([' ', '\t'])));
        if eq_trimmed {
            starts.push(start);
        }
    }

    starts
}

/// Remplace `search` par `replace` dans `source`, de façon tolérante.
///
/// Stratégie en deux passes :
///
/// 1. **Exact** : si `search` apparaît une seule fois littéralement, il est
///    remplacé tel quel. Plus d'une occurrence ⇒ `Err(Ambiguous)`.
/// 2. **Fuzzy (indentation)** : `\r\n` est normalisé en `\n` puis les deux
///    blocs sont dédentés relativement. Le bloc est retrouvé même si le
///    fichier l'indente différemment (4, 8, … espaces) ; `replace` est
///    ré-indenté avec l'indentation de base du bloc trouvé, et les fins de
///    ligne d'origine (`\r\n` si `source` est en CRLF) sont préservées.
///    Plus d'une correspondance floue ⇒ `Err(Ambiguous)` : jamais
///    d'ambiguïté silencieuse.
///
/// # Erreurs
///
/// Le type d'erreur est [`MatchResult`] lui-même, avec
/// `status = Ambiguous` (plusieurs candidats) ou `status = NotFound`
/// (aucun candidat), et un `context` explicatif.
pub fn apply_patch(source: &str, search: &str, replace: &str) -> Result<String, MatchResult> {
    if search.trim().is_empty() {
        return Err(error(MatchStatus::NotFound, "recherche vide".to_string()));
    }

    // ---- Passe 1 : correspondance exacte ----
    let occurrences = source.matches(search).count();
    if occurrences == 1 {
        return Ok(source.replacen(search, replace, 1));
    }
    if occurrences > 1 {
        return Err(error(
            MatchStatus::Ambiguous,
            format!(
                "recherche exacte présente {} fois — spécifiez un contexte plus large",
                occurrences
            ),
        ));
    }

    // ---- Passe 1.5 : correspondance exacte après normalisation CRLF <-> LF ----
    let uses_crlf = source.contains("\r\n");
    let source_lf = source.replace("\r\n", "\n");
    let search_lf = search.replace("\r\n", "\n");
    let replace_lf = replace.replace("\r\n", "\n");

    let lf_occurrences = source_lf.matches(&search_lf).count();
    if lf_occurrences == 1 {
        let patched_lf = source_lf.replacen(&search_lf, &replace_lf, 1);
        return Ok(if uses_crlf {
            patched_lf.replace('\n', "\r\n")
        } else {
            patched_lf
        });
    }
    if lf_occurrences > 1 {
        return Err(error(
            MatchStatus::Ambiguous,
            format!(
                "recherche présente {} fois (fins de ligne normalisées) — spécifiez un contexte plus large",
                lf_occurrences
            ),
        ));
    }

    // ---- Passe 2 : fallback normalisé (indentation + espaces de fin de ligne) ----
    let source_lines: Vec<&str> = source_lf.lines().collect();
    let search_lines: Vec<&str> = search_lf.lines().collect();

    let starts = fuzzy_matches(&source_lines, &search_lines);

    if starts.is_empty() {
        return Err(error(
            MatchStatus::NotFound,
            format!(
                "aucune correspondance, même après normalisation d'indentation, dans {} lignes",
                source_lines.len()
            ),
        ));
    }

    if starts.len() > 1 {
        let lines: Vec<usize> = starts.iter().map(|s| s + 1).collect();
        return Err(error(
            MatchStatus::Ambiguous,
            format!(
                "correspondance approximative ambiguë : {} occurrences (lignes {:?})",
                starts.len(),
                lines
            ),
        ));
    }

    let start = starts[0];
    let matched_window = &source_lines[start..start + search_lines.len()];
    let window_min_line = matched_window
        .iter()
        .copied()
        .filter(|l| !l.trim().is_empty())
        .min_by_key(|l| indent_len(l))
        .unwrap_or("");
    let window_min = indent_len(window_min_line);
    let search_min = search_lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_len(l))
        .min()
        .unwrap_or(0);
    let indent_unit = if window_min_line.starts_with('\t') {
        '\t'
    } else {
        ' '
    };
    let delta: isize = (window_min as isize) - (search_min as isize);

    let replace_lines: Vec<&str> = replace_lf.lines().collect();
    let mut new_lines: Vec<String> = Vec::with_capacity(replace_lines.len());
    for line in replace_lines {
        if line.trim().is_empty() {
            new_lines.push(String::new());
        } else if delta > 0 {
            let pad: String = std::iter::repeat_n(indent_unit, delta as usize).collect();
            new_lines.push(format!("{}{}", pad, line));
        } else if delta < 0 {
            let strip = ((-delta) as usize).min(indent_len(line));
            new_lines.push(line[strip..].to_string());
        } else {
            new_lines.push(line.to_string());
        }
    }

    // Reconstruction en préservant l'ordre et les fins de ligne d'origine.
    let mut out: Vec<String> = Vec::with_capacity(source_lines.len() + new_lines.len());
    out.extend(source_lines[..start].iter().map(|s| (*s).to_string()));
    out.extend(new_lines);
    out.extend(
        source_lines[start + search_lines.len()..]
            .iter()
            .map(|s| (*s).to_string()),
    );

    let eol = if uses_crlf { "\r\n" } else { "\n" };
    let mut result = out.join(eol);
    if source_lf.ends_with('\n') {
        result.push_str(eol);
    }

    Ok(result)
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

    #[test]
    fn apply_patch_exact_replaces_once() {
        let source = "alpha\nbeta\ngamma\n";
        let out = apply_patch(source, "beta", "BETA").expect("remplacement exact attendu");
        assert_eq!(out, "alpha\nBETA\ngamma\n");
    }

    #[test]
    fn apply_patch_exact_ambiguous_is_refused() {
        let source = "x = 1;\ny = 2;\nx = 1;\n";
        let err = apply_patch(source, "x = 1;", "x = 9;").expect_err("ambiguïté exacte attendue");
        assert_eq!(err.status, MatchStatus::Ambiguous);
        assert!(err.is_error());
    }

    #[test]
    fn apply_patch_fuzzy_indent_shift() {
        // Le search est écrit sans indentation, le fichier indente de 4 espaces.
        let source = "fn main() {\n    let x = 1;\n    let y = 2;\n}\n";
        let out = apply_patch(source, "let x = 1;\nlet y = 2;", "let z = 3;\nlet w = 4;")
            .expect("fallback indentation attendu");
        assert_eq!(out, "fn main() {\n    let z = 3;\n    let w = 4;\n}\n");
    }

    #[test]
    fn apply_patch_preserves_crlf() {
        let source = "a\r\n    keep\r\nb\r\n";
        let out = apply_patch(source, "keep", "KEEP").expect("remplacement attendu");
        assert_eq!(out, "a\r\n    KEEP\r\nb\r\n");
        assert!(!out.replace("\r\n", "").contains('\n'), "aucun LF isolé");
    }

    #[test]
    fn apply_patch_fuzzy_ambiguous_is_refused() {
        let source = "    dup();\n    other();\n    dup();\n";
        let err = apply_patch(source, "dup();", "done();").expect_err("ambiguïté fuzzy attendue");
        assert_eq!(err.status, MatchStatus::Ambiguous);
    }

    #[test]
    fn apply_patch_multiline_crlf_with_closing_braces_and_trailing_spaces() {
        let source = "mod tests {\r\n    fn demo() {\r\n        assert!(true);   \r\n    }\r\n}\r\n";
        let search = "        assert!(true);\n    }\n}";
        let replace = "        assert!(true);\n    }\n\n    fn extra() {}\n}";
        let out = apply_patch(source, search, replace).expect("doit matcher malgré CRLF + espaces finaux");
        assert_eq!(
            out,
            "mod tests {\r\n    fn demo() {\r\n        assert!(true);\r\n    }\r\n\r\n    fn extra() {}\r\n}\r\n"
        );
    }
}
