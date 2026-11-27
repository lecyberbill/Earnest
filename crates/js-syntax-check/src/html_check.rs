//! Extraction des blocs `<script>` inline d'un HTML et verification syntaxique.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::js_check;
use crate::CheckResult;

/// Un bloc `<script>` inline extrait d'un HTML.
#[derive(Debug, Clone)]
pub struct InlineScript {
    /// Contenu JS brut du bloc.
    pub content: String,
    /// Ligne (1-indexee) du premier caractere du contenu dans le HTML.
    pub content_start_line: usize,
}

/// Verifie tous les blocs `<script>` inline (sans `src=`) d'un document HTML.
pub fn check_html(html: &str) -> CheckResult {
    let scripts = extract_inline_scripts(html);
    let mut checked = 0usize;
    for script in &scripts {
        checked += 1;
        let tmp = TempFile::new(&script.content);
        let outcome = js_check::run_node_check(tmp.path());
        if !outcome.ok {
            let line = outcome
                .error_line
                .map(|l| script.content_start_line + l.saturating_sub(1));
            return CheckResult {
                ok: false,
                scripts_checked: checked,
                error: outcome.error,
                line,
            };
        }
    }
    CheckResult {
        ok: true,
        scripts_checked: checked,
        error: None,
        line: None,
    }
}

/// Extrait tous les blocs `<script>...</script>` inline (sans attribut `src=`),
/// dans l'ordre d'apparition.
pub fn extract_inline_scripts(html: &str) -> Vec<InlineScript> {
    let mut scripts = Vec::new();
    let lower = html.to_lowercase();
    let total = html.len();
    let mut pos = 0usize;

    while pos < total {
        let rest = &lower[pos..];
        let tag_rel = match rest.find("<script") {
            Some(r) => r,
            None => break,
        };
        let tag_start = pos + tag_rel;
        let after_tag = tag_start + "<script".len();

        // `<scriptaire...` ne compte pas : le caractere suivant doit etre
        // un separateur d'attributs, `>` ou `/`.
        let next = html[after_tag..].chars().next().unwrap_or('\0');
        if !(next == '>' || next == '/' || next.is_ascii_whitespace()) {
            pos = tag_start + 1;
            continue;
        }

        let gt_rel = match html[tag_start..].find('>') {
            Some(r) => r,
            None => break,
        };
        let gt = tag_start + gt_rel;
        let tag = html[tag_start..=gt].to_lowercase();

        let has_src = tag
            .split_whitespace()
            .any(|tok| tok == "src" || tok.starts_with("src="));

        if !has_src {
            let content_start = gt + 1;
            if let Some(close_rel) = lower[content_start..].find("</script") {
                let content_end = content_start + close_rel;
                scripts.push(InlineScript {
                    content: html[content_start..content_end].to_string(),
                    content_start_line: line_of_offset(html, content_start),
                });
            }
        }
        pos = gt + 1;
    }

    scripts
}

/// Ligne (1-indexee) de l'octet situe a l'index `offset` dans `s`.
fn line_of_offset(s: &str, offset: usize) -> usize {
    debug_assert!(offset <= s.len());
    s[..offset].bytes().filter(|b| *b == b'\n').count() + 1
}

/// Fichier temporaire, supprime automatiquement a la fin du scope.
struct TempFile(PathBuf);

impl TempFile {
    fn new(content: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "js_syntax_check_{}_{}.js",
            std::process::id(),
            n
        ));
        std::fs::write(&path, content).expect("ecriture du fichier temporaire");
        TempFile(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_inline_et_src() {
        let html = "<html><body>\n<script>const a=1;</script>\n<script src=\"app.js\"></script>\n<script>const b=2;</script>\n</body></html>";
        let s = extract_inline_scripts(html);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].content, "const a=1;");
        assert_eq!(s[1].content, "const b=2;");
        // Ligne du premier caractere du contenu = 2 pour le premier bloc
        assert_eq!(s[0].content_start_line, 2);
    }

    #[test]
    fn extraction_majuscules() {
        let html = "<BODY><SCRIPT>\nconst x = 1;\n</SCRIPT></BODY>";
        let s = extract_inline_scripts(html);
        assert_eq!(s.len(), 1);
        assert!(s[0].content.contains("const x"));
    }
}
