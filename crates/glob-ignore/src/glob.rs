//! Glob pattern parsing and matching.
//!
//! A [`GlobPattern`] is a compiled pattern that can be matched against a
//! `/`-separated path. The matcher is fully iterative: it never recurses and
//! never backtracks exponentially, so pathological patterns such as
//! `a*a*a*a*a*a*b` run in `O(N * M)` time.
//!
//! # Supported syntax
//!
//! | Token      | Meaning                                                        |
//! |------------|----------------------------------------------------------------|
//! | `*`        | Any sequence of characters except `/` (possibly empty).        |
//! | `**`       | Zero or more whole path segments.                              |
//! | `?`        | Exactly one character except `/`.                              |
//! | `[abc]`    | One character from the set.                                    |
//! | `[a-z]`    | One character from the range.                                  |
//! | `[!a-z]`   | One character **not** in the set (`[^a-z]` is equivalent).     |
//! | `\x`       | The literal character `x`.                                     |
//!
//! Windows separators (`\`) in the *subject path* are normalised to `/` before
//! matching. Inside a *pattern*, `\` is the escape character, so a literal
//! backslash must be written `\\`.

use crate::error::{GlobError, GlobErrorKind};

/// A single compiled token of a glob pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// A literal character that must match exactly.
    Literal(char),
    /// `?`: exactly one character, never `/`.
    AnyChar,
    /// `*`: any run of characters, never `/`.
    AnySegmentChars,
    /// `**`: zero or more whole path segments.
    AnySegments,
    /// A character class such as `[a-z]` or `[!0-9]`.
    Class(CharClass),
}

/// A compiled character class.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CharClass {
    /// Whether the class is negated (`[!...]` or `[^...]`).
    negated: bool,
    /// Inclusive ranges of members.
    ranges: Vec<(char, char)>,
}

impl CharClass {
    /// Returns `true` when `c` is a member of the class.
    fn matches(&self, c: char) -> bool {
        let inside = self
            .ranges
            .iter()
            .any(|&(lo, hi)| lo <= c && c <= hi);
        inside != self.negated
    }
}

/// A compiled glob pattern.
///
/// Instances are created with [`GlobPattern::parse`] and matched with
/// [`GlobPattern::matches`].
///
/// # Examples
///
/// ```
/// use glob_ignore::GlobPattern;
///
/// let pattern = GlobPattern::parse("src/**/*.rs").unwrap();
/// assert!(pattern.matches("src/main.rs"));
/// assert!(pattern.matches("src/a/b/lib.rs"));
/// assert!(!pattern.matches("tests/main.rs"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobPattern {
    /// The original pattern text, kept for diagnostics.
    source: String,
    /// The compiled token stream.
    tokens: Vec<Token>,
}

impl GlobPattern {
    /// Compiles `pattern` into a [`GlobPattern`].
    ///
    /// # Errors
    ///
    /// Returns a [`GlobError`] when the pattern contains a syntax error such as
    /// an unterminated character class, an empty class, a reversed range, a
    /// misplaced `**`, or a dangling escape.
    ///
    /// # Examples
    ///
    /// ```
    /// use glob_ignore::GlobPattern;
    ///
    /// assert!(GlobPattern::parse("*.rs").is_ok());
    /// assert!(GlobPattern::parse("[abc").is_err());
    /// ```
    pub fn parse(pattern: &str) -> Result<Self, GlobError> {
        let tokens = compile(pattern)?;
        Ok(Self {
            source: pattern.to_string(),
            tokens,
        })
    }

    /// Returns the original pattern text this instance was compiled from.
    ///
    /// ```
    /// use glob_ignore::GlobPattern;
    ///
    /// let pattern = GlobPattern::parse("a/*.rs").unwrap();
    /// assert_eq!(pattern.as_str(), "a/*.rs");
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// Returns `true` when `path` matches this pattern.
    ///
    /// Windows separators in `path` are normalised to `/` before matching.
    /// Leading `./` prefixes and duplicate separators are collapsed.
    ///
    /// # Examples
    ///
    /// ```
    /// use glob_ignore::GlobPattern;
    ///
    /// let pattern = GlobPattern::parse("a/**/b").unwrap();
    /// assert!(pattern.matches("a/b"));
    /// assert!(pattern.matches("a/x/y/b"));
    /// assert!(pattern.matches("a\\x\\b"));
    /// assert!(!pattern.matches("a/x/y/c"));
    /// ```
    #[must_use]
    pub fn matches(&self, path: &str) -> bool {
        let normalised = normalise_path(path);
        match_tokens(&self.tokens, &normalised)
    }
}

/// Normalises a subject path: `\` becomes `/`, `./` prefixes are dropped and
/// repeated separators are collapsed. Never panics.
pub(crate) fn normalise_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut last_was_sep = false;
    for c in path.chars() {
        let c = if c == '\\' { '/' } else { c };
        if c == '/' {
            if last_was_sep {
                continue;
            }
            last_was_sep = true;
        } else {
            last_was_sep = false;
        }
        out.push(c);
    }
    // Strip a leading `./` (possibly repeated) and any leading `/`.
    loop {
        if let Some(rest) = out.strip_prefix("./") {
            out = rest.to_string();
        } else if let Some(rest) = out.strip_prefix('/') {
            out = rest.to_string();
        } else {
            break;
        }
    }
    // Strip a trailing `/`.
    while out.ends_with('/') {
        out.pop();
    }
    out
}

/// Normalises Windows separators inside a *pattern*.
///
/// A `\` that precedes a character which is meaningful to the glob or
/// `.gitignore` syntax (`*`, `?`, `[`, `]`, `\`, `#`, space, tab) — or that
/// ends the pattern — is kept as an escape sequence. Any other `\` is treated
/// as a Windows path separator and rewritten to `/`. This makes
/// `src\**\*.rs` equivalent to `src/**/*.rs` while preserving `a\*b` as the
/// literal `*`, `foo\ ` as a trailing space and `\#file` as a literal `#`.
fn normalise_pattern(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len());
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            match chars.get(i + 1) {
                Some(&next)
                    if matches!(next, '*' | '?' | '[' | ']' | '\\' | '#' | ' ' | '\t') =>
                {
                    out.push('\\');
                    out.push(next);
                    i += 2;
                }
                None => {
                    // Dangling escape: keep it so `compile` can report it.
                    out.push('\\');
                    i += 1;
                }
                _ => {
                    out.push('/');
                    i += 1;
                }
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Compiles a pattern into a token stream.
fn compile(pattern: &str) -> Result<Vec<Token>, GlobError> {
    let chars: Vec<char> = normalise_pattern(pattern).chars().collect();
    let mut tokens = Vec::with_capacity(chars.len());
    let mut i = 0usize;
    let len = chars.len();

    while i < len {
        let c = chars[i];
        match c {
            '\\' => {
                // Escape: the next character is literal.
                if i + 1 >= len {
                    return Err(GlobError::new(
                        GlobErrorKind::DanglingEscape,
                        pattern,
                        i,
                    ));
                }
                tokens.push(Token::Literal(chars[i + 1]));
                i += 2;
            }
            '*' => {
                if i + 1 < len && chars[i + 1] == '*' {
                    // `**` must occupy a whole segment: preceded by start or
                    // `/`, and followed by end or `/`.
                    let prev_ok = i == 0 || chars[i - 1] == '/';
                    let next_ok = i + 2 >= len || chars[i + 2] == '/';
                    if !prev_ok || !next_ok {
                        return Err(GlobError::new(
                            GlobErrorKind::MisplacedDoubleStar,
                            pattern,
                            i,
                        ));
                    }
                    // The `AnySegments` token owns the separators around it, so
                    // drop the `/` that precedes it (already emitted as a
                    // literal) to keep `foo/**` matching `foo` itself.
                    if matches!(tokens.last(), Some(Token::Literal('/'))) {
                        tokens.pop();
                    }
                    tokens.push(Token::AnySegments);
                    i += 2;
                    // Skip the separator that follows `**`, if any: the
                    // `AnySegments` token already accounts for it.
                    if i < len && chars[i] == '/' {
                        i += 1;
                    }
                } else {
                    tokens.push(Token::AnySegmentChars);
                    i += 1;
                }
            }
            '?' => {
                tokens.push(Token::AnyChar);
                i += 1;
            }
            '[' => {
                let (class, next) = parse_class(&chars, i, pattern)?;
                tokens.push(Token::Class(class));
                i = next;
            }
            _ => {
                tokens.push(Token::Literal(c));
                i += 1;
            }
        }
    }

    Ok(tokens)
}

/// Parses a character class starting at `start` (which points at `[`).
///
/// Returns the compiled class and the index just past the closing `]`.
fn parse_class(
    chars: &[char],
    start: usize,
    pattern: &str,
) -> Result<(CharClass, usize), GlobError> {
    let len = chars.len();
    let mut i = start + 1;
    let mut negated = false;

    if i < len && (chars[i] == '!' || chars[i] == '^') {
        negated = true;
        i += 1;
    }

    let mut ranges: Vec<(char, char)> = Vec::new();

    // A `]` immediately after the opening `[` (or after `!`/`^`) closes an
    // empty class: `[]` and `[!]` are syntax errors, not literal brackets.
    if i < len && chars[i] == ']' {
        return Err(GlobError::new(GlobErrorKind::EmptyClass, pattern, start));
    }

    while i < len {
        let c = chars[i];
        if c == ']' {
            if ranges.is_empty() {
                return Err(GlobError::new(GlobErrorKind::EmptyClass, pattern, start));
            }
            return Ok((CharClass { negated, ranges }, i + 1));
        }

        // Read a single member, honouring escapes.
        let lo = if c == '\\' {
            if i + 1 >= len {
                return Err(GlobError::new(
                    GlobErrorKind::DanglingEscape,
                    pattern,
                    i,
                ));
            }
            i += 1;
            chars[i]
        } else {
            c
        };
        i += 1;

        // Look for a range `lo-hi`.
        if i + 1 < len && chars[i] == '-' && chars[i + 1] != ']' {
            let mut j = i + 1;
            let hi = if chars[j] == '\\' {
                if j + 1 >= len {
                    return Err(GlobError::new(
                        GlobErrorKind::DanglingEscape,
                        pattern,
                        j,
                    ));
                }
                j += 1;
                chars[j]
            } else {
                chars[j]
            };
            if hi < lo {
                return Err(GlobError::new(
                    GlobErrorKind::InvalidRange,
                    pattern,
                    i,
                ));
            }
            ranges.push((lo, hi));
            i = j + 1;
        } else {
            ranges.push((lo, lo));
        }
    }

    Err(GlobError::new(
        GlobErrorKind::UnterminatedClass,
        pattern,
        start,
    ))
}

/// Matches a compiled token stream against a normalised path.
///
/// The algorithm is the classic two-pointer wildcard matcher extended with
/// `**` support. It keeps track of the most recent `*`/`**` so that it can
/// resume from there instead of backtracking recursively. Complexity is
/// `O(tokens * path)` in the worst case.
///
/// `AnySegments` (`**`) may match **zero** segments, in which case it simply
/// disappears from the token stream (the separators around it were already
/// absorbed at compile time). When it does consume input, it always consumes
/// whole segments, i.e. it stops right after a `/`.
fn match_tokens(tokens: &[Token], path: &str) -> bool {
    let text: Vec<char> = path.chars().collect();
    let mut ti = 0usize; // token index
    let mut pi = 0usize; // path index

    // Backtracking state for the most recent `*` (single-segment).
    let mut star_ti: Option<usize> = None;
    let mut star_pi = 0usize;

    // Backtracking state for the most recent `**` (multi-segment).
    let mut dstar_ti: Option<usize> = None;
    let mut dstar_pi = 0usize;

    loop {
        if ti < tokens.len() {
            match &tokens[ti] {
                Token::Literal(c) => {
                    if pi < text.len() && text[pi] == *c {
                        ti += 1;
                        pi += 1;
                        continue;
                    }
                }
                Token::AnyChar => {
                    if pi < text.len() && text[pi] != '/' {
                        ti += 1;
                        pi += 1;
                        continue;
                    }
                }
                Token::Class(class) => {
                    if pi < text.len() && text[pi] != '/' && class.matches(text[pi]) {
                        ti += 1;
                        pi += 1;
                        continue;
                    }
                }
                Token::AnySegmentChars => {
                    star_ti = Some(ti);
                    star_pi = pi;
                    ti += 1;
                    continue;
                }
                Token::AnySegments => {
                    // Zero-segment match: skip the token without consuming
                    // input. The greedy extension is handled on mismatch.
                    dstar_ti = Some(ti);
                    dstar_pi = pi;
                    ti += 1;
                    continue;
                }
            }
        } else if pi >= text.len() {
            // All tokens consumed and all input consumed.
            return true;
        }

        // Mismatch (or input exhausted with tokens left): backtrack.
        if let Some(sti) = star_ti {
            if star_pi < text.len() && text[star_pi] != '/' {
                star_pi += 1;
                pi = star_pi;
                ti = sti + 1;
                continue;
            }
        }
        if let Some(dti) = dstar_ti {
            if dstar_pi < text.len() {
                // Consume one whole segment: advance to just past the next
                // `/`, or to the end of the input.
                let mut next = dstar_pi;
                while next < text.len() && text[next] != '/' {
                    next += 1;
                }
                if next < text.len() {
                    next += 1; // consume the `/`
                }
                if next > dstar_pi {
                    dstar_pi = next;
                    pi = dstar_pi;
                    ti = dti + 1;
                    continue;
                }
            }
        }
        return false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pattern: &str) -> GlobPattern {
        GlobPattern::parse(pattern).expect("pattern should compile")
    }

    #[test]
    fn parse_literal_and_wildcards() {
        assert!(GlobPattern::parse("abc").is_ok());
        assert!(GlobPattern::parse("a*b?c").is_ok());
        assert!(GlobPattern::parse("**/foo").is_ok());
        assert!(GlobPattern::parse("foo/**").is_ok());
        assert!(GlobPattern::parse("a/**/b").is_ok());
    }

    #[test]
    fn parse_classes_and_escapes() {
        assert!(GlobPattern::parse("[abc]").is_ok());
        assert!(GlobPattern::parse("[a-z]").is_ok());
        assert!(GlobPattern::parse("[!a-z]").is_ok());
        assert!(GlobPattern::parse("[^a-z]").is_ok());
        assert!(GlobPattern::parse(r"a\*b").is_ok());
    }

    #[test]
    fn parse_errors() {
        assert_eq!(
            GlobPattern::parse("[abc").unwrap_err().kind(),
            GlobErrorKind::UnterminatedClass
        );
        assert_eq!(
            GlobPattern::parse("[]").unwrap_err().kind(),
            GlobErrorKind::EmptyClass
        );
        assert_eq!(
            GlobPattern::parse("[z-a]").unwrap_err().kind(),
            GlobErrorKind::InvalidRange
        );
        assert_eq!(
            GlobPattern::parse("a**b").unwrap_err().kind(),
            GlobErrorKind::MisplacedDoubleStar
        );
        assert_eq!(
            GlobPattern::parse("abc\\").unwrap_err().kind(),
            GlobErrorKind::DanglingEscape
        );
    }

    #[test]
    fn match_star_does_not_cross_separator() {
        let pattern = p("a/*/c");
        assert!(pattern.matches("a/b/c"));
        assert!(!pattern.matches("a/b/d/c"));
    }

    #[test]
    fn match_double_star_zero_or_more_segments() {
        let pattern = p("a/**/b");
        assert!(pattern.matches("a/b"));
        assert!(pattern.matches("a/x/b"));
        assert!(pattern.matches("a/x/y/b"));
        assert!(!pattern.matches("a/x/y/c"));
    }

    #[test]
    fn match_leading_and_trailing_double_star() {
        let leading = p("**/foo");
        assert!(leading.matches("foo"));
        assert!(leading.matches("a/foo"));
        assert!(leading.matches("a/b/foo"));

        let trailing = p("foo/**");
        assert!(trailing.matches("foo"));
        assert!(trailing.matches("foo/a"));
        assert!(trailing.matches("foo/a/b"));
        assert!(!trailing.matches("bar/a"));
    }

    #[test]
    fn match_question_mark() {
        let pattern = p("a?c");
        assert!(pattern.matches("abc"));
        assert!(!pattern.matches("a/c"));
        assert!(!pattern.matches("ac"));
    }

    #[test]
    fn match_character_classes() {
        let pattern = p("file[0-9].txt");
        assert!(pattern.matches("file3.txt"));
        assert!(!pattern.matches("filex.txt"));

        let negated = p("file[!0-9].txt");
        assert!(negated.matches("filex.txt"));
        assert!(!negated.matches("file3.txt"));

        let caret = p("file[^0-9].txt");
        assert!(caret.matches("filex.txt"));
        assert!(!caret.matches("file3.txt"));
    }

    #[test]
    fn match_escaped_metacharacters() {
        let pattern = p(r"a\*b");
        assert!(pattern.matches("a*b"));
        assert!(!pattern.matches("axb"));
    }

    #[test]
    fn match_windows_separators() {
        let pattern = p("a/b/c");
        assert!(pattern.matches(r"a\b\c"));
        assert!(pattern.matches("./a/b/c"));
        assert!(pattern.matches("a//b//c"));
    }

    #[test]
    fn match_unicode_is_safe() {
        let pattern = p("héllo/*");
        assert!(pattern.matches("héllo/wörld"));
        assert!(!pattern.matches("hello/wörld"));
    }

    #[test]
    fn no_redos_on_pathological_pattern() {
        // `a*a*a*a*a*a*a*a*a*a*b` against a long run of `a`s must terminate
        // quickly thanks to the iterative matcher.
        let pattern = p("a*a*a*a*a*a*a*a*a*a*b");
        let subject = "a".repeat(200);
        let start = std::time::Instant::now();
        assert!(!pattern.matches(&subject));
        assert!(start.elapsed().as_secs() < 2);
    }

    #[test]
    fn no_redos_on_pathological_double_star() {
        let pattern = p("**/**/**/**/**/**/**/z");
        let subject = "a/".repeat(100) + "y";
        let start = std::time::Instant::now();
        assert!(!pattern.matches(&subject));
        assert!(start.elapsed().as_secs() < 2);
    }

    #[test]
    fn no_redos_matches_when_it_should() {
        let pattern = p("a*a*a*a*a*a*a*a*a*a*b");
        let subject = "a".repeat(200) + "b";
        let start = std::time::Instant::now();
        assert!(pattern.matches(&subject));
        assert!(start.elapsed().as_secs() < 2);
    }

    #[test]
    fn normalise_path_handles_edge_cases() {
        assert_eq!(normalise_path(""), "");
        assert_eq!(normalise_path("/"), "");
        assert_eq!(normalise_path("./a/./b"), "a/./b");
        assert_eq!(normalise_path(r"a\b\c"), "a/b/c");
        assert_eq!(normalise_path("a///b"), "a/b");
        assert_eq!(normalise_path("a/b/"), "a/b");
    }
}
