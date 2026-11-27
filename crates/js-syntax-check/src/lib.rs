//! # js-syntax-check
//!
//! Verifie la syntaxe de fichiers JavaScript (`.js`) et des blocs
//! `<script>` inline dans des fichiers HTML en utilisant `node --check`,
//! sans aucune dependance externe (uniquement la std lib).

pub mod html_check;
pub mod js_check;

use std::path::Path;

/// Resultat de la verification syntaxique d'un fichier `.js` ou `.html`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    /// `true` si la syntaxe est valide.
    pub ok: bool,
    /// Nombre de scripts verifies (1 pour un `.js`, nombre de blocs
    /// `<script>` inline pour un `.html`).
    pub scripts_checked: usize,
    /// Message d'erreur (si `ok == false`).
    pub error: Option<String>,
    /// Numero de ligne (1-indexe) de l'erreur dans le fichier d'origine.
    pub line: Option<usize>,
}

impl CheckResult {
    /// Serialise le resultat en JSON (sans dependance externe).
    ///
    /// # Exemple
    ///
    /// ```
    /// use js_syntax_check::CheckResult;
    /// let r = CheckResult {
    ///     ok: true,
    ///     scripts_checked: 1,
    ///     error: None,
    ///     line: None,
    /// };
    /// let json = r.to_json();
    /// assert_eq!(json, r#"{"ok": true, "scripts_checked": 1, "error": null, "line": null}"#);
    /// ```
    pub fn to_json(&self) -> String {
        let mut s = String::with_capacity(96);
        s.push_str("{\"ok\": ");
        s.push_str(&self.ok.to_string());
        s.push_str(", \"scripts_checked\": ");
        s.push_str(&self.scripts_checked.to_string());
        s.push_str(", \"error\": ");
        match &self.error {
            Some(e) => s.push_str(&json_escape(e)),
            None => s.push_str("null"),
        }
        s.push_str(", \"line\": ");
        match self.line {
            Some(l) => s.push_str(&l.to_string()),
            None => s.push_str("null"),
        }
        s.push('}');
        s
    }
}

/// Verifie la syntaxe d'un fichier `.js` ou `.html`.
///
/// - `.js` : execute `node --check <fichier>`.
/// - `.html` : extrait chaque bloc `<script>...</script>` inline (sans
///   attribut `src=`), le verifie via `node --check` sur un fichier
///   temporaire et rapporte la ligne reelle dans le HTML.
pub fn check_file(path: &Path) -> CheckResult {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "js" | "mjs" | "cjs" => {
            let o = js_check::run_node_check(path);
            CheckResult {
                ok: o.ok,
                scripts_checked: 1,
                error: o.error,
                line: o.error_line,
            }
        }
        "html" | "htm" => {
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    return CheckResult {
                        ok: false,
                        scripts_checked: 0,
                        error: Some(format!("lecture du fichier impossible : {e}")),
                        line: None,
                    }
                }
            };
            html_check::check_html(&content)
        }
        other => CheckResult {
            ok: false,
            scripts_checked: 0,
            error: Some(format!(
                "extension non prise en charge : {other} (attendu .js ou .html)"
            )),
            line: None,
        },
    }
}

/// Echappe une chaine pour l'insérer dans une chaine JSON.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
