//! Error types produced by the crate.
//!
//! Every fallible operation in `glob_ignore` returns [`GlobError`]. The type is
//! deliberately small: it only describes *syntax* problems found while parsing
//! a pattern. Matching itself is infallible and never panics.

use std::fmt;

/// The kind of syntax error detected while parsing a glob pattern.
///
/// The enum is `#[non_exhaustive]` so that new diagnostics can be added in a
/// backwards-compatible way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GlobErrorKind {
    /// A `[` character class was opened but never closed.
    UnterminatedClass,
    /// A character class such as `[]` or `[!]` contains no member.
    EmptyClass,
    /// A character class range is reversed, e.g. `[z-a]`.
    InvalidRange,
    /// A `**` wildcard appears where it cannot be interpreted, e.g. `a**b`.
    MisplacedDoubleStar,
    /// The pattern ends with a lone escape character `\`.
    DanglingEscape,
}

impl GlobErrorKind {
    /// Returns a short, stable, human-readable description of the error kind.
    ///
    /// ```
    /// use glob_ignore::GlobErrorKind;
    ///
    /// assert_eq!(GlobErrorKind::EmptyClass.as_str(), "empty character class");
    /// ```
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnterminatedClass => "unterminated character class",
            Self::EmptyClass => "empty character class",
            Self::InvalidRange => "invalid character class range",
            Self::MisplacedDoubleStar => "misplaced `**` wildcard",
            Self::DanglingEscape => "dangling escape sequence",
        }
    }
}

/// A syntax error found while parsing a glob pattern.
///
/// The error keeps the original pattern and the byte offset at which the
/// problem was detected, which makes diagnostics actionable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobError {
    kind: GlobErrorKind,
    pattern: String,
    offset: usize,
}

impl GlobError {
    /// Builds a new error.
    ///
    /// ```
    /// use glob_ignore::{GlobError, GlobErrorKind};
    ///
    /// let err = GlobError::new(GlobErrorKind::DanglingEscape, "a\\", 1);
    /// assert_eq!(err.kind(), GlobErrorKind::DanglingEscape);
    /// ```
    #[must_use]
    pub fn new(kind: GlobErrorKind, pattern: &str, offset: usize) -> Self {
        Self {
            kind,
            pattern: pattern.to_owned(),
            offset,
        }
    }

    /// Returns the kind of syntax error.
    #[must_use]
    pub fn kind(&self) -> GlobErrorKind {
        self.kind
    }

    /// Returns the pattern that failed to parse.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Returns the byte offset inside the pattern where the error was found.
    #[must_use]
    pub fn offset(&self) -> usize {
        self.offset
    }
}

impl fmt::Display for GlobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at offset {} in pattern `{}`",
            self.kind.as_str(),
            self.offset,
            self.pattern
        )
    }
}

impl std::error::Error for GlobError {}

#[cfg(test)]
mod tests {
    use super::{GlobError, GlobErrorKind};

    #[test]
    fn error_kind_as_str_is_stable() {
        assert_eq!(
            GlobErrorKind::UnterminatedClass.as_str(),
            "unterminated character class"
        );
        assert_eq!(GlobErrorKind::EmptyClass.as_str(), "empty character class");
        assert_eq!(
            GlobErrorKind::InvalidRange.as_str(),
            "invalid character class range"
        );
        assert_eq!(
            GlobErrorKind::MisplacedDoubleStar.as_str(),
            "misplaced `**` wildcard"
        );
        assert_eq!(
            GlobErrorKind::DanglingEscape.as_str(),
            "dangling escape sequence"
        );
    }

    #[test]
    fn accessors_and_display() {
        let err = GlobError::new(GlobErrorKind::EmptyClass, "a[]b", 2);
        assert_eq!(err.kind(), GlobErrorKind::EmptyClass);
        assert_eq!(err.pattern(), "a[]b");
        assert_eq!(err.offset(), 2);
        let rendered = err.to_string();
        assert!(rendered.contains("empty character class"));
        assert!(rendered.contains("a[]b"));
        assert!(rendered.contains('2'));
    }

    #[test]
    fn error_implements_std_error() {
        fn assert_error<E: std::error::Error>(_: &E) {}
        let err = GlobError::new(GlobErrorKind::DanglingEscape, "a\\", 1);
        assert_error(&err);
    }
}
