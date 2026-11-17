//! Workspace symbol scanner and fuzzy search index.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Supported symbol categories extracted from source files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    Class,
    Interface,
    TypeAlias,
    Export,
    Module,
}

/// A single indexed symbol with its file path and 1-based line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolEntry {
    pub name: String,
    pub kind: SymbolKind,
    pub file: PathBuf,
    pub line: usize,
    pub signature: String,
}

/// In-memory index of symbols extracted from a workspace directory.
#[derive(Debug, Default, Clone)]
pub struct SymbolIndex {
    entries: Vec<SymbolEntry>,
}

const IGNORED_DIRS: &[&str] = &[
    ".git",
    ".ide",
    ".venv",
    "__pycache__",
    "build",
    "dist",
    "node_modules",
    "target",
];

const SUPPORTED_EXTS: &[&str] = &[
    "rs", "py", "js", "jsx", "ts", "tsx", "mjs", "cjs", "go", "java", "kt", "c", "h", "cpp", "hpp",
];

impl SymbolIndex {
    /// Creates an empty symbol index.
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Recursively indexes all supported source files under `root`.
    pub fn build(root: &Path) -> io::Result<Self> {
        let mut index = Self::new();
        index.scan_dir(root, root)?;
        Ok(index)
    }

    /// Returns all indexed symbols.
    pub fn entries(&self) -> &[SymbolEntry] {
        &self.entries
    }

    /// Returns the total number of indexed symbols.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if no symbols have been indexed.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Indexes a single file in memory (useful for incremental updates or tests).
    pub fn index_file(&mut self, root: &Path, path: &Path) -> io::Result<()> {
        let content = fs::read_to_string(path)?;
        let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        self.entries.retain(|e| e.file != rel);
        self.parse_source(&rel, &content);
        Ok(())
    }

    /// Searches indexed symbols using exact, prefix, substring, and fuzzy subsequence scoring.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&SymbolEntry> {
        let q = query.trim().to_lowercase();
        if q.is_empty() || limit == 0 {
            return Vec::new();
        }

        let mut scored: Vec<(i32, &SymbolEntry)> = self
            .entries
            .iter()
            .filter_map(|entry| score_symbol(&entry.name.to_lowercase(), &q).map(|s| (s, entry)))
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.len().cmp(&b.1.name.len())).then_with(|| a.1.line.cmp(&b.1.line)));
        scored.into_iter().take(limit).map(|(_, e)| e).collect()
    }

    fn scan_dir(&mut self, root: &Path, dir: &Path) -> io::Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if path.is_dir() {
                if IGNORED_DIRS.contains(&name_str.as_ref()) || name_str.starts_with('.') {
                    continue;
                }
                self.scan_dir(root, &path)?;
            } else if is_supported_file(&path) {
                if let Ok(content) = fs::read_to_string(&path) {
                    let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                    self.parse_source(&rel, &content);
                }
            }
        }
        Ok(())
    }

    fn parse_source(&mut self, rel_path: &Path, content: &str) {
        for (idx, raw_line) in content.lines().enumerate() {
            let line_num = idx + 1;
            let trimmed = raw_line.trim();
            if trimmed.is_empty()
                || trimmed.starts_with("//")
                || trimmed.starts_with('#')
                || trimmed.starts_with("/*")
                || trimmed.starts_with('*')
            {
                continue;
            }
            if let Some((kind, name)) = extract_symbol(trimmed) {
                self.entries.push(SymbolEntry {
                    name,
                    kind,
                    file: rel_path.to_path_buf(),
                    line: line_num,
                    signature: trimmed.to_string(),
                });
            }
        }
    }
}

fn is_supported_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| SUPPORTED_EXTS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn extract_symbol(line: &str) -> Option<(SymbolKind, String)> {
    // Strip common visibility / async / export prefixes iteratively.
    let mut s = line;
    let mut saw_export = false;
    loop {
        let prev = s;
        for prefix in &[
            "pub(crate) ",
            "pub(super) ",
            "pub ",
            "export default ",
            "export ",
            "async ",
            "unsafe ",
            "const ",
            "declare ",
            "abstract ",
        ] {
            if let Some(rest) = s.strip_prefix(prefix) {
                if *prefix == "export " || *prefix == "export default " {
                    saw_export = true;
                }
                s = rest.trim_start();
            }
        }
        if s == prev {
            break;
        }
    }

    let rules: &[(&str, SymbolKind)] = &[
        ("fn ", SymbolKind::Function),
        ("def ", SymbolKind::Function),
        ("function ", SymbolKind::Function),
        ("func ", SymbolKind::Function),
        ("struct ", SymbolKind::Struct),
        ("enum ", SymbolKind::Enum),
        ("trait ", SymbolKind::Trait),
        ("impl ", SymbolKind::Impl),
        ("class ", SymbolKind::Class),
        ("interface ", SymbolKind::Interface),
        ("type ", SymbolKind::TypeAlias),
        ("mod ", SymbolKind::Module),
    ];

    for (kw, kind) in rules {
        if let Some(rest) = s.strip_prefix(kw) {
            let ident = take_ident(rest)?;
            return Some((*kind, ident));
        }
    }

    // Handle `export const foo = ...` / `export let bar = ...`
    if saw_export {
        for kw in &["let ", "var "] {
            if let Some(rest) = s.strip_prefix(kw) {
                let ident = take_ident(rest)?;
                return Some((SymbolKind::Export, ident));
            }
        }
        // `const ` was stripped in the loop above; if it's `export const FOO = ...`, grab identifier.
        if let Some(ident) = take_ident(s) {
            return Some((SymbolKind::Export, ident));
        }
    }

    None
}

fn take_ident(s: &str) -> Option<String> {
    let trimmed = s.trim_start();
    // Handle `impl<T> Foo` or `impl Trait for Type` by skipping generic `<...>` if needed.
    let start = if trimmed.starts_with('<') {
        let end = trimmed.find('>')?;
        trimmed[end + 1..].trim_start()
    } else {
        trimmed
    };

    let ident: String = start
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if ident.is_empty() {
        None
    } else {
        Some(ident)
    }
}

fn score_symbol(name: &str, query: &str) -> Option<i32> {
    if name == query {
        return Some(1000);
    }
    if name.starts_with(query) {
        return Some(800 - (name.len() as i32 - query.len() as i32).min(200));
    }
    if let Some(pos) = name.find(query) {
        return Some(600 - (pos as i32).min(200));
    }
    // Subsequence fuzzy match
    let mut qi = query.chars().peekable();
    let mut gaps = 0i32;
    let mut started = false;
    for ch in name.chars() {
        if let Some(&qc) = qi.peek() {
            if ch == qc {
                qi.next();
                started = true;
            } else if started {
                gaps += 1;
            }
        } else {
            break;
        }
    }
    if qi.peek().is_none() {
        Some((400 - gaps * 5).max(50))
    } else {
        None
    }
}
