//! `.gitignore`-compatible rule matching.
//!
//! [`IgnoreMatcher`] compiles the textual content of a `.gitignore` file into an
//! ordered list of rules and answers the question *"is this path ignored?"*.
//!
//! # Rule syntax
//!
//! * Blank lines are ignored.
//! * Lines starting with `#` are comments (escape a leading `#` with `\#`).
//! * Trailing unescaped whitespace is stripped; escape a trailing space with
//!   `\ ` to keep it.
//! * A leading `!` negates the rule (re-inclusion).
//! * A leading `/` anchors the pattern to the root of the tree.
//! * A trailing `/` restricts the rule to directories.
//! * A pattern containing no `/` (other than a trailing one) matches the
//!   *basename* at any depth.
//! * The **last** matching rule wins.
//!
//! # Re-inclusion caveat
//!
//! Git never re-includes a file whose parent directory is excluded. This crate
//! reproduces that behaviour: [`IgnoreMatcher::is_ignored`] checks every ancestor
//! directory of the path and reports the path as ignored when any ancestor is
//! itself ignored.

use crate::error::GlobError;
use crate::glob::{normalise_path, GlobPattern};

/// A single compiled `.gitignore` rule.
#[derive(Debug, Clone)]
struct Rule {
    /// The compiled glob pattern.
    pattern: GlobPattern,
    /// `true` when the rule starts with `!` (re-inclusion).
    negated: bool,
    /// `true` when the rule ends with `/` (directories only).
    dir_only: bool,
    /// `true` when the rule is anchored to the root (leading `/` or an
    /// embedded `/`).
    anchored: bool,
}

impl Rule {
    /// Returns `true` when this rule applies to `path`.
    fn applies(&self, path: &str, is_dir: bool) -> bool {
        if self.dir_only && !is_dir {
            return false;
        }
        if self.anchored {
            self.pattern.matches(path)
        } else {
            // Unanchored patterns match the basename at any depth.
            let basename = path.rsplit('/').next().unwrap_or(path);
            self.pattern.matches(basename)
        }
    }
}

/// A compiled set of `.gitignore` rules.
///
/// # Examples
///
/// ```
/// use glob_ignore::IgnoreMatcher;
///
/// let matcher = IgnoreMatcher::from_gitignore("*.log\n!important.log\n").unwrap();
/// assert!(matcher.is_ignored("debug.log", false));
/// assert!(!matcher.is_ignored("important.log", false));
/// assert!(!matcher.is_ignored("src/main.rs", false));
/// ```
#[derive(Debug, Clone)]
pub struct IgnoreMatcher {
    /// Rules in file order; the last applicable one wins.
    rules: Vec<Rule>,
}

impl IgnoreMatcher {
    /// Compiles the textual content of a `.gitignore` file.
    ///
    /// # Errors
    ///
    /// Returns [`GlobError`] when a non-comment line contains an invalid glob
    /// pattern (unterminated class, reversed range, dangling escape, ...).
    ///
    /// # Examples
    ///
    /// ```
    /// use glob_ignore::IgnoreMatcher;
    ///
    /// let matcher = IgnoreMatcher::from_gitignore("# comment\n\nbuild/\n").unwrap();
    /// assert!(matcher.is_ignored("build", true));
    /// assert!(!matcher.is_ignored("build", false));
    /// ```
    pub fn from_gitignore(content: &str) -> Result<Self, GlobError> {
        let mut rules = Vec::new();
        for raw_line in content.lines() {
            if let Some(rule) = Self::parse_line(raw_line)? {
                rules.push(rule);
            }
        }
        Ok(Self { rules })
    }

    /// Parses a single `.gitignore` line.
    ///
    /// Returns `Ok(None)` for blank lines and comments.
    fn parse_line(raw_line: &str) -> Result<Option<Rule>, GlobError> {
        // Strip a trailing `\r` left over from CRLF input.
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);

        // A line starting with `#` is a comment unless the `#` is escaped.
        if line.starts_with('#') {
            return Ok(None);
        }

        // Strip trailing unescaped whitespace.
        let line = strip_trailing_whitespace(line);
        if line.is_empty() {
            return Ok(None);
        }

        // Detect and strip a leading `!` (negation).
        let (negated, line) = match line.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        if line.is_empty() {
            return Ok(None);
        }

        // Detect and strip a trailing `/` (directories only).
        let (dir_only, line) = match line.strip_suffix('/') {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        if line.is_empty() {
            return Ok(None);
        }

        // Detect and strip a leading `/` (anchored to the root).
        let (leading_slash, line) = match line.strip_prefix('/') {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        if line.is_empty() {
            return Ok(None);
        }

        // A pattern is anchored when it starts with `/` or contains an
        // unescaped `/` somewhere in the middle.
        let anchored = leading_slash || contains_unescaped_slash(line);

        let pattern = GlobPattern::parse(line)?;
        Ok(Some(Rule {
            pattern,
            negated,
            dir_only,
            anchored,
        }))
    }

    /// Returns `true` when `rel_path` is ignored.
    ///
    /// `rel_path` is interpreted relative to the root of the tree and may use
    /// either `/` or `\` as separator. `is_dir` tells whether the path denotes a
    /// directory.
    ///
    /// A path is ignored when the last applicable rule is a non-negated one.
    /// Additionally, a path is ignored when any of its ancestor directories is
    /// ignored, mirroring Git's behaviour (a file cannot be re-included when its
    /// parent directory is excluded).
    ///
    /// # Examples
    ///
    /// ```
    /// use glob_ignore::IgnoreMatcher;
    ///
    /// let matcher = IgnoreMatcher::from_gitignore("target/\n!target/keep\n").unwrap();
    /// // `target` is excluded, so nothing below it can be re-included.
    /// assert!(matcher.is_ignored("target/keep", false));
    /// ```
    #[must_use]
    pub fn is_ignored(&self, rel_path: &str, is_dir: bool) -> bool {
        let path = normalise_path(rel_path);
        if path.is_empty() {
            return false;
        }

        // Check ancestors first: an excluded parent directory wins.
        let mut prefix_end = 0usize;
        for (idx, byte) in path.bytes().enumerate() {
            if byte == b'/' {
                let ancestor = &path[..idx];
                if !ancestor.is_empty() && self.decide(ancestor, true) {
                    return true;
                }
                prefix_end = idx + 1;
            }
        }
        let _ = prefix_end;

        self.decide(&path, is_dir)
    }

    /// Applies the last-match-wins rule to a single path, without ancestor
    /// checks.
    fn decide(&self, path: &str, is_dir: bool) -> bool {
        let mut ignored = false;
        for rule in &self.rules {
            if rule.applies(path, is_dir) {
                ignored = !rule.negated;
            }
        }
        ignored
    }

    /// Returns the number of compiled rules.
    ///
    /// # Examples
    ///
    /// ```
    /// use glob_ignore::IgnoreMatcher;
    ///
    /// let matcher = IgnoreMatcher::from_gitignore("*.log\n# comment\n\nbuild/\n").unwrap();
    /// assert_eq!(matcher.len(), 2);
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Returns `true` when no rule was compiled.
    ///
    /// # Examples
    ///
    /// ```
    /// use glob_ignore::IgnoreMatcher;
    ///
    /// let matcher = IgnoreMatcher::from_gitignore("# only comments\n").unwrap();
    /// assert!(matcher.is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// Strips trailing whitespace that is not escaped by a backslash.
fn strip_trailing_whitespace(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut end = bytes.len();
    while end > 0 {
        let last = bytes[end - 1];
        if last != b' ' && last != b'\t' {
            break;
        }
        // Count the backslashes immediately preceding this whitespace char.
        let mut backslashes = 0usize;
        let mut idx = end - 1;
        while idx > 0 && bytes[idx - 1] == b'\\' {
            backslashes += 1;
            idx -= 1;
        }
        if backslashes % 2 == 1 {
            // The whitespace is escaped: keep it (and the escape) as-is.
            break;
        }
        end -= 1;
    }
    &line[..end]
}

/// Returns `true` when `line` contains a `/` that is not escaped by `\`.
fn contains_unescaped_slash(line: &str) -> bool {
    let mut escaped = false;
    for c in line.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '/' => return true,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::IgnoreMatcher;

    fn matcher(content: &str) -> IgnoreMatcher {
        IgnoreMatcher::from_gitignore(content).expect("valid gitignore")
    }

    #[test]
    fn gitignore_comments_and_blank_lines() {
        let m = matcher("# a comment\n\n   \n*.log\n");
        assert_eq!(m.len(), 1);
        assert!(m.is_ignored("a.log", false));
        assert!(!m.is_ignored("a.txt", false));
    }

    #[test]
    fn gitignore_trailing_whitespace_is_stripped() {
        let m = matcher("foo.txt   \n");
        assert!(m.is_ignored("foo.txt", false));
        assert!(!m.is_ignored("foo.txt   ", false));
    }

    #[test]
    fn gitignore_escaped_trailing_space_is_kept() {
        let m = matcher("foo\\ \n");
        assert!(m.is_ignored("foo ", false));
        assert!(!m.is_ignored("foo", false));
    }

    #[test]
    fn gitignore_negation_reincludes() {
        let m = matcher("*.log\n!important.log\n");
        assert!(m.is_ignored("debug.log", false));
        assert!(!m.is_ignored("important.log", false));
    }

    #[test]
    fn gitignore_anchored_pattern() {
        let m = matcher("/build\n");
        assert!(m.is_ignored("build", true));
        assert!(!m.is_ignored("src/build", true));
    }

    #[test]
    fn gitignore_dir_only_pattern() {
        let m = matcher("build/\n");
        assert!(m.is_ignored("build", true));
        assert!(!m.is_ignored("build", false));
    }

    #[test]
    fn gitignore_basename_matches_at_any_depth() {
        let m = matcher("*.tmp\n");
        assert!(m.is_ignored("a.tmp", false));
        assert!(m.is_ignored("deep/nested/b.tmp", false));
    }

    #[test]
    fn gitignore_embedded_slash_is_anchored() {
        let m = matcher("src/gen\n");
        assert!(m.is_ignored("src/gen", true));
        assert!(!m.is_ignored("other/src/gen", true));
    }

    #[test]
    fn gitignore_windows_separators_in_subject() {
        let m = matcher("src/gen\n");
        assert!(m.is_ignored(r"src\gen", true));
    }

    #[test]
    fn gitignore_crlf_input() {
        let m = matcher("*.log\r\n!keep.log\r\n");
        assert!(m.is_ignored("a.log", false));
        assert!(!m.is_ignored("keep.log", false));
    }

    #[test]
    fn gitignore_empty_matcher() {
        let m = matcher("");
        assert!(m.is_empty());
        assert!(!m.is_ignored("anything", false));
    }

    #[test]
    fn gitignore_invalid_pattern_is_reported() {
        assert!(IgnoreMatcher::from_gitignore("[abc\n").is_err());
    }

    #[test]
    fn gitignore_escaped_hash_is_literal() {
        let m = matcher("\\#file\n");
        assert!(m.is_ignored("#file", false));
    }
}
