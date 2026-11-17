//! Nettoyage des **fences Markdown** et du **BOM** avant extraction de la charge utile.
//!
//! ## Règles
//!
//! - BOM U+FEFF en début de chaîne : supprimé.
//! - Fences ` ``` ` ou ` ~~~ ` ancrés en **début de ligne** (<= 3 espaces de
//!   retrait) avec un info-string optionnel (ex: `json`, `JSON`, `JSON5`) :
//!   les deux lignes de délimitation sont supprimées, le contenu entre elles
//!   est conservé.
//! - Si un fence d'ouverture est trouvé mais qu'aucun fence de fermeture
//!   n'existe, le fence d'ouverture seul est supprimé.
//! - Les fences sont ignorés s'ils sont à l'intérieur d'une chaîne JSON
//!   (détection de parité de guillemets hors-fence).
//! - Aucune copie : les fonctions retournent des sous-tranches `&str`.

/// Supprime le BOM U+FEFF éventuel en début de chaîne.
pub(crate) fn strip_bom(input: &str) -> &str {
    if let Some(rest) = input.strip_prefix('\u{feff}') {
        rest
    } else {
        input
    }
}

/// Supprime les fences Markdown (``` / ~~~) d'une entrée.
///
/// Retourne une sous-tranche de `input` (zéro copie).
/// Si un seul fence d'ouverture existe sans fermeture, il est supprimé seul.
pub(crate) fn strip_markdown_fences(input: &str) -> &str {
    // Trouver le premier fence d'ouverture ancré en début de ligne
    let (fence_char, _open_start, open_end, after_open) = match find_open_fence(input) {
        Some(f) => f,
        None => return input,
    };

    // Chercher le fence de fermeture correspondant (même char, même longueur >= 3).
    // `find_close_fence` retourne un offset RELATIF à `after_open` : on le
    // convertit en offset absolu dans `input` en ajoutant `open_end`.
    if let Some(close_rel) = find_close_fence(after_open, fence_char) {
        let content_start = open_end;
        let content_end = open_end + close_rel;
        if content_start > content_end {
            // Cas pathologique : close avant open — ne rien faire
            return input;
        }
        // Le contenu entre les fences est la sous-tranche. On retire les blancs
        // de tête ET de queue : le `\n` qui précède la ligne de fermeture fait
        // partie de la tranche et ne doit pas polluer la charge utile.
        input[content_start..content_end].trim()
    } else {
        // Pas de fence de fermeture : supprimer seulement le fence d'ouverture
        // (le contenu qui suit est conservé tel quel)
        input[open_end..].trim_start()
    }
}

/// Trouve le premier fence d'ouverture ancré en début de ligne.
///
/// Retourne `(char_du_fence, index_début_ligne, index_fin_ligne, contenu_après)`.
fn find_open_fence(input: &str) -> Option<(char, usize, usize, &str)> {
    let mut line_start = 0;
    for (i, line) in input.split_inclusive('\n').enumerate() {
        let line_end = line_start + line.len();
        // Ignorer les lignes vides ou avec uniquement des espaces
        let trimmed = line.trim_start();
        let leading = line.len() - trimmed.len();
        if leading <= 3 && (trimmed.starts_with("```") || trimmed.starts_with("~~~")) {
            let fence_char = if trimmed.starts_with("```") { '`' } else { '~' };
            // Vérifier que ce n'est PAS à l'intérieur d'une chaîne JSON
            // (parité de guillemets avant cette ligne)
            let before = &input[..line_start];
            if is_fence_outside_string(before) {
                // Le fence est valide — retourne les indices
                // après_open = tout ce qui suit la fin de la ligne du fence
                let after = &input[line_end..];
                return Some((fence_char, line_start, line_end, after));
            }
        }
        line_start = line_end;
        let _ = i;
    }
    None
}

/// Trouve le fence de fermeture correspondant (même char, >= 3, ancré en début de ligne,
/// sans info-string ou avec une info-string vide).
fn find_close_fence(after_open: &str, fence_char: char) -> Option<usize> {
    let mut line_start = 0;
    for line in after_open.split_inclusive('\n') {
        let line_end = line_start + line.len();
        let trimmed = line.trim();
        // Fence de fermeture : exactement 3+ du même char, rien d'autre sur la ligne
        // (pas d'info-string pour la fermeture)
        let is_fence = fence_char == '`' && trimmed.starts_with("```") && trimmed.chars().all(|c| c == '`');
        let is_fence_tilde = fence_char == '~' && trimmed.starts_with("~~~") && trimmed.chars().all(|c| c == '~');
        if is_fence || is_fence_tilde {
            return Some(line_start);
        }
        line_start = line_end;
    }
    None
}

/// Vérifie par parité de guillemets si un fence est à l'extérieur d'une chaîne JSON.
///
/// On ne compte que les guillemets `"` précédés d'un nombre PAIR de backslashes.
fn is_fence_outside_string(before: &str) -> bool {
    let mut in_string = false;
    let mut backslash_count = 0u32;
    for ch in before.chars() {
        match ch {
            '\\' => backslash_count += 1,
            '"' => {
                if backslash_count % 2 == 0 {
                    in_string = !in_string;
                }
                backslash_count = 0;
            }
            _ => {
                backslash_count = 0;
            }
        }
    }
    !in_string
}

// ---------------------------------------------------------------------------
// Tests unitaires — C9 (fence, prose, markdown)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fence_simple_json_block() {
        let input = "```json\n{\"a\": 1}\n```";
        let result = strip_markdown_fences(input);
        assert_eq!(result, "{\"a\": 1}");
    }

    #[test]
    fn fence_tilde_block() {
        let input = "~~~json\n[1, 2, 3]\n~~~";
        let result = strip_markdown_fences(input);
        assert_eq!(result, "[1, 2, 3]");
    }

    #[test]
    fn fence_no_info_string() {
        let input = "```\n{\"x\": true}\n```";
        let result = strip_markdown_fences(input);
        assert_eq!(result, "{\"x\": true}");
    }

    #[test]
    fn fence_with_prose_around() {
        let input = "Voici la réponse :\n```json\n{\"nom\": \"Alice\"}\n```\nMerci !";
        // strip_markdown_fences extrait le contenu entre les fences
        let result = strip_markdown_fences(input);
        // La prose avant le fence n'est pas dans la sous-tranche entre fences
        assert_eq!(result, "{\"nom\": \"Alice\"}");
    }

    #[test]
    fn fence_opening_only_no_closing() {
        let input = "```json\n{\"a\": 1}";
        let result = strip_markdown_fences(input);
        assert_eq!(result, "{\"a\": 1}");
    }

    #[test]
    fn fence_with_newlines_inside() {
        let input = "```json\n{\n  \"a\": 1,\n  \"b\": 2\n}\n```";
        let result = strip_markdown_fences(input);
        assert!(result.contains("\"a\": 1"));
        assert!(result.contains("\"b\": 2"));
    }

    #[test]
    fn fence_indented_leading_spaces() {
        let input = "   ```json\n{\"a\": 1}\n   ```";
        let result = strip_markdown_fences(input);
        assert_eq!(result, "{\"a\": 1}");
    }

    #[test]
    fn strip_bom_removes_bom() {
        let input = "\u{feff}{\"a\": 1}";
        assert_eq!(strip_bom(input), "{\"a\": 1}");
    }

    #[test]
    fn strip_bom_no_bom() {
        let input = "{\"a\": 1}";
        assert_eq!(strip_bom(input), "{\"a\": 1}");
    }

    #[test]
    fn fence_markdown_in_string_not_stripped() {
        // Les ``` à l'intérieur d'une chaîne JSON ne doivent pas être traités comme fence
        let input = r#"{"code": "```json"}"#;
        let result = strip_markdown_fences(input);
        // Pas de fence d'ouverture ancré en début de ligne hors chaîne → retour identique
        assert_eq!(result, r#"{"code": "```json"}"#);
    }
}