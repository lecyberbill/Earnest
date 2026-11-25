//! Error type returned by the `.env` parser.

use std::fmt;

/// Errors produced while parsing a `.env` document.
///
/// Every variant carries the 1-based line number where the problem was
/// detected so that callers can point the user at the offending line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DotenvError {
    /// The key part of an assignment is not a valid identifier.
    ///
    /// A valid key starts with an ASCII letter or `_` and is followed by
    /// ASCII letters, digits or `_`.
    InvalidKey {
        /// 1-based line number of the offending assignment.
        line: usize,
        /// The raw key text that failed validation.
        key: String,
    },
    /// A quoted value was opened but never closed.
    UnterminatedQuote {
        /// 1-based line number where the quote was opened.
        line: usize,
        /// The quote character that was left open (`'` or `"`).
        quote: char,
    },
    /// The input contains a NUL byte (`\0`), which is never valid in a `.env`
    /// file.
    NulByte {
        /// 1-based line number containing the NUL byte.
        line: usize,
    },
    /// A line is neither blank, a comment, nor a `KEY=VALUE` assignment.
    MissingAssignment {
        /// 1-based line number of the offending line.
        line: usize,
    },
    /// A double-quoted value ends with a dangling backslash escape.
    DanglingEscape {
        /// 1-based line number of the offending line.
        line: usize,
    },
}

impl DotenvError {
    /// Returns the 1-based line number associated with this error.
    ///
    /// ```
    /// use dotenv_shield::parse_dotenv;
    ///
    /// let err = parse_dotenv("1BAD=value").unwrap_err();
    /// assert_eq!(err.line(), 1);
    /// ```
    #[must_use]
    pub fn line(&self) -> usize {
        match self {
            Self::InvalidKey { line, .. }
            | Self::UnterminatedQuote { line, .. }
            | Self::NulByte { line }
            | Self::MissingAssignment { line }
            | Self::DanglingEscape { line } => *line,
        }
    }
}

impl fmt::Display for DotenvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey { line, key } => {
                write!(f, "line {line}: invalid key `{key}`")
            }
            Self::UnterminatedQuote { line, quote } => {
                write!(f, "line {line}: unterminated {quote} quote")
            }
            Self::NulByte { line } => {
                write!(f, "line {line}: NUL byte is not allowed")
            }
            Self::MissingAssignment { line } => {
                write!(f, "line {line}: expected `KEY=VALUE` assignment")
            }
            Self::DanglingEscape { line } => {
                write!(f, "line {line}: dangling escape sequence")
            }
        }
    }
}

impl std::error::Error for DotenvError {}

#[cfg(test)]
mod tests {
    use super::DotenvError;

    #[test]
    fn display_and_line_accessor() {
        let err = DotenvError::InvalidKey {
            line: 3,
            key: "1BAD".to_string(),
        };
        assert_eq!(err.line(), 3);
        assert_eq!(err.to_string(), "line 3: invalid key `1BAD`");

        let err = DotenvError::UnterminatedQuote {
            line: 7,
            quote: '"',
        };
        assert_eq!(err.line(), 7);
        assert!(err.to_string().contains("unterminated"));

        let err = DotenvError::NulByte { line: 2 };
        assert_eq!(err.line(), 2);

        let err = DotenvError::MissingAssignment { line: 4 };
        assert_eq!(err.line(), 4);

        let err = DotenvError::DanglingEscape { line: 5 };
        assert_eq!(err.line(), 5);
    }
}
