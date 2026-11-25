//! Segmentation d'une ligne de commande shell.
//!
//! Le decoupage est conscient :
//! - des quotes simples `'...'` et doubles `"..."` (aucune coupure a l'interieur),
//! - des echappements POSIX `\` et PowerShell `` ` `` (le caractere suivant est litteral),
//! - des sous-commandes `$( ... )` (segmentation recursive bornee),
//! - des separateurs `;`, `&&`, `||`, `|` et retours a la ligne.
//!
//! Aucune entree ne provoque de panic : une quote non fermee produit simplement
//! un segment final brut.

/// Profondeur maximale de recursion pour les sous-commandes `$( ... )`.
pub const MAX_SUBSHELL_DEPTH: usize = 8;

/// Un segment de ligne de commande (une « commande simple »).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Segment {
    /// Texte brut du segment, separateurs exclus, sous-commandes incluses in-line.
    pub raw: String,
    /// Sous-commandes `$( ... )` trouvees dans ce segment (possiblement imbriquees).
    pub subshells: Vec<Segment>,
    /// Vrai si ce segment recoit l'entree du segment precedent via un pipe `|`.
    pub piped_from: bool,
}

impl Segment {
    /// Cree un segment vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Vrai si le segment ne contient ni texte utile ni sous-commande.
    pub fn is_blank(&self) -> bool {
        self.raw.trim().is_empty() && self.subshells.is_empty()
    }

    /// Vrai si le segment est alimente en entree par un pipe.
    pub fn receives_piped_input(&self) -> bool {
        self.piped_from
    }

    /// Liste aplatie (recursive) des sous-commandes de ce segment.
    pub fn all_subshells(&self) -> Vec<&Segment> {
        let mut out = Vec::new();
        for sub in &self.subshells {
            out.push(sub);
            out.extend(sub.all_subshells());
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuoteState {
    Normal,
    Single,
    Double,
    EscapePosix,
    EscapePs,
}

/// Decoupe une ligne de commande en segments.
pub fn split_segments(input: &str) -> Vec<Segment> {
    split_inner(input, 0)
}

fn split_inner(input: &str, depth: usize) -> Vec<Segment> {
    let chars: Vec<char> = input.chars().collect();
    let mut segments: Vec<Segment> = Vec::new();
    let mut cur = Segment::new();
    let mut cur_piped = false;
    let mut state = QuoteState::Normal;
    let mut i = 0usize;

    while i < chars.len() {
        let c = chars[i];
        match state {
            QuoteState::EscapePosix | QuoteState::EscapePs => {
                // Le caractere echappe est litteral : il ne peut pas separer.
                cur.raw.push(c);
                state = QuoteState::Normal;
                i += 1;
            }
            QuoteState::Single => {
                cur.raw.push(c);
                if c == '\'' {
                    state = QuoteState::Normal;
                }
                i += 1;
            }
            QuoteState::Double => match c {
                '"' => {
                    cur.raw.push(c);
                    state = QuoteState::Normal;
                    i += 1;
                }
                '\\' => {
                    cur.raw.push(c);
                    state = QuoteState::EscapePosix;
                    i += 1;
                }
                '`' => {
                    cur.raw.push(c);
                    state = QuoteState::EscapePs;
                    i += 1;
                }
                '$' if chars.get(i + 1) == Some(&'(') => {
                    i = consume_subshell(&chars, i, depth, &mut cur);
                }
                _ => {
                    cur.raw.push(c);
                    i += 1;
                }
            },
            QuoteState::Normal => match c {
                '\\' => {
                    cur.raw.push(c);
                    state = QuoteState::EscapePosix;
                    i += 1;
                }
                '`' => {
                    cur.raw.push(c);
                    state = QuoteState::EscapePs;
                    i += 1;
                }
                '\'' => {
                    cur.raw.push(c);
                    state = QuoteState::Single;
                    i += 1;
                }
                '"' => {
                    cur.raw.push(c);
                    state = QuoteState::Double;
                    i += 1;
                }
                ';' | '\n' => {
                    flush(&mut segments, &mut cur, cur_piped);
                    cur_piped = false;
                    i += 1;
                }
                '&' if chars.get(i + 1) == Some(&'&') => {
                    flush(&mut segments, &mut cur, cur_piped);
                    cur_piped = false;
                    i += 2;
                }
                '|' if chars.get(i + 1) == Some(&'|') => {
                    flush(&mut segments, &mut cur, cur_piped);
                    cur_piped = false;
                    i += 2;
                }
                '|' => {
                    flush(&mut segments, &mut cur, cur_piped);
                    cur_piped = true;
                    i += 1;
                }
                '$' if chars.get(i + 1) == Some(&'(') => {
                    i = consume_subshell(&chars, i, depth, &mut cur);
                }
                _ => {
                    cur.raw.push(c);
                    i += 1;
                }
            },
        }
    }

    // Segment final : emis meme si une quote est restee ouverte.
    flush(&mut segments, &mut cur, cur_piped);
    segments
}

fn flush(segments: &mut Vec<Segment>, cur: &mut Segment, piped: bool) {
    if cur.is_blank() {
        cur.raw.clear();
        cur.subshells.clear();
        return;
    }
    let mut seg = std::mem::replace(cur, Segment::new());
    seg.piped_from = piped;
    segments.push(seg);
}

/// Consomme une sous-commande `$( ... )` a partir de `dollar_idx`.
/// Retourne l'index juste apres la parenthese fermante.
fn consume_subshell(chars: &[char], dollar_idx: usize, depth: usize, cur: &mut Segment) -> usize {
    match take_subshell(chars, dollar_idx) {
        Some((end, inner)) => {
            for k in dollar_idx..=end {
                if let Some(ch) = chars.get(k) {
                    cur.raw.push(*ch);
                }
            }
            if depth < MAX_SUBSHELL_DEPTH && !inner.trim().is_empty() {
                let nested = split_inner(&inner, depth + 1);
                cur.subshells.extend(nested);
            }
            end + 1
        }
        None => {
            cur.raw.push('$');
            dollar_idx + 1
        }
    }
}

/// Index de la parenthese fermante correspondant au `$(` situe en `dollar_idx`.
pub(crate) fn matching_paren(chars: &[char], dollar_idx: usize) -> Option<usize> {
    take_subshell(chars, dollar_idx).map(|(end, _)| end)
}

/// Extrait le contenu d'une sous-commande `$( ... )` et l'index de sa parenthese fermante.
fn take_subshell(chars: &[char], dollar_idx: usize) -> Option<(usize, String)> {
    if chars.get(dollar_idx) != Some(&'$') || chars.get(dollar_idx + 1) != Some(&'(') {
        return None;
    }
    let mut level = 1usize;
    let mut j = dollar_idx + 2;
    let mut state = QuoteState::Normal;
    let mut inner = String::new();

    while j < chars.len() {
        let c = chars[j];
        match state {
            QuoteState::EscapePosix | QuoteState::EscapePs => {
                inner.push(c);
                state = QuoteState::Normal;
                j += 1;
            }
            QuoteState::Single => {
                inner.push(c);
                if c == '\'' {
                    state = QuoteState::Normal;
                }
                j += 1;
            }
            QuoteState::Double => {
                if c == '"' {
                    state = QuoteState::Normal;
                    inner.push(c);
                } else if c == '\\' {
                    state = QuoteState::EscapePosix;
                    inner.push(c);
                } else if c == '`' {
                    state = QuoteState::EscapePs;
                    inner.push(c);
                } else {
                    if c == '(' {
                        level += 1;
                    } else if c == ')' {
                        level -= 1;
                        if level == 0 {
                            return Some((j, inner));
                        }
                    }
                    inner.push(c);
                }
                j += 1;
            }
            QuoteState::Normal => {
                match c {
                    '\\' => {
                        state = QuoteState::EscapePosix;
                        inner.push(c);
                    }
                    '`' => {
                        state = QuoteState::EscapePs;
                        inner.push(c);
                    }
                    '\'' => {
                        state = QuoteState::Single;
                        inner.push(c);
                    }
                    '"' => {
                        state = QuoteState::Double;
                        inner.push(c);
                    }
                    '(' => {
                        level += 1;
                        inner.push(c);
                    }
                    ')' => {
                        level -= 1;
                        if level == 0 {
                            return Some((j, inner));
                        }
                        inner.push(c);
                    }
                    _ => inner.push(c),
                }
                j += 1;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raws(input: &str) -> Vec<String> {
        split_segments(input)
            .iter()
            .map(|s| s.raw.trim().to_string())
            .collect()
    }

    #[test]
    fn test_segmentation_quotes_echappements_souscommandes() {
        // --- separateurs simples ---
        assert_eq!(raws("ls -la && cat file"), vec!["ls -la", "cat file"]);
        assert_eq!(raws("a; b; c"), vec!["a", "b", "c"]);
        assert_eq!(raws("a || b"), vec!["a", "b"]);
        assert_eq!(raws("a\nb"), vec!["a", "b"]);
        assert_eq!(raws("ls | grep x"), vec!["ls", "grep x"]);

        // --- quotes : aucune coupure interne ---
        assert_eq!(raws("echo \"a;b && c\""), vec!["echo \"a;b && c\""]);
        assert_eq!(raws("echo 'x | y'"), vec!["echo 'x | y'"]);
        assert_eq!(raws("echo 'a;b' ; echo c"), vec!["echo 'a;b'", "echo c"]);
        assert_eq!(raws("ls \"a\nb\""), vec!["ls \"a\nb\""]);

        // --- echappements POSIX `\` ---
        assert_eq!(raws("echo a\\;b"), vec!["echo a\\;b"]);
        assert_eq!(raws("echo a\\|b"), vec!["echo a\\|b"]);
        // continuation de ligne : pas de coupure
        assert_eq!(raws("echo a \\\n b"), vec!["echo a \\\n b"]);

        // --- echappement PowerShell (backtick) ---
        assert_eq!(raws("Write-Host a`;b"), vec!["Write-Host a`;b"]);
        assert_eq!(raws("Write-Host a`|b"), vec!["Write-Host a`|b"]);

        // --- sous-commandes `$( ... )` ---
        let segs = split_segments("cat $(echo a; echo b) | grep x");
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].subshells.len(), 2);
        assert_eq!(segs[0].subshells[0].raw.trim(), "echo a");
        assert_eq!(segs[0].subshells[1].raw.trim(), "echo b");
        assert!(segs[1].piped_from);
        assert!(!segs[0].piped_from);

        // sous-commande dans des doubles quotes
        let segs = split_segments("echo \"$(ls; pwd)\"");
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].subshells.len(), 2);

        // imbrication
        let nested = split_segments("echo $(echo $(echo ok))");
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0].subshells.len(), 1);
        assert_eq!(nested[0].subshells[0].subshells.len(), 1);
        assert_eq!(
            nested[0].subshells[0].subshells[0].raw.trim(),
            "echo ok"
        );

        // profondeur bornee : pas de stack overflow
        let deep = format!("echo {}ok{}", "$(".repeat(40), ")".repeat(40));
        let segs = split_segments(&deep);
        assert_eq!(segs.len(), 1);

        // --- robustesse : quotes non fermees, entrees vides ---
        assert_eq!(raws("echo \"abc"), vec!["echo \"abc"]);
        assert_eq!(raws("echo 'abc"), vec!["echo 'abc"]);
        assert_eq!(raws("echo abc\\"), vec!["echo abc\\"]);
        assert!(split_segments("").is_empty());
        assert!(split_segments("   \n\t  ").is_empty());
        assert_eq!(raws(";;;"), Vec::<String>::new());
    }
}
