//! Error types for the `expr_eval` expression engine.
//!
//! Every error carries the source position (byte `offset`, 1-based `line` and
//! 1-based `col`) so that business rules can be reported with precision.
//!
//! # Examples
//!
//! ```
//! use expr_eval::{compile, EvalContext, ExprError};
//!
//! match compile("1 +") {
//!     Ok(_) => unreachable!(),
//!     Err(e) => println!("{e}"),
//! }
//! ```

use std::fmt;

/// The maximum parser recursion depth accepted by the engine.
///
/// Expressions nesting deeper than this are rejected with
/// [`ExprError::RecursionLimitExceeded`] before any evaluation takes place.
pub const MAX_RECURSION_DEPTH: usize = 64;

/// A structured error produced either while parsing an expression or while
/// evaluating a compiled expression.
///
/// All variants implement [`std::error::Error`], [`Clone`], [`Eq`] and
/// [`PartialEq`] so that they can be matched on, compared, and logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprError {
    /// An unexpected character was encountered during lexing.
    ///
    /// Fields: `character`, `offset`, `line`, `col`.
    UnexpectedChar {
        /// The offending character (as a single-char string).
        character: char,
        /// Zero-based byte offset in the source.
        offset: usize,
        /// One-based line number.
        line: usize,
        /// One-based column number (in characters).
        col: usize,
    },
    /// A string literal was opened but never closed.
    ///
    /// Fields: `offset`, `line`, `col`.
    UnterminatedString {
        /// Zero-based byte offset of the opening quote.
        offset: usize,
        /// One-based line number.
        line: usize,
        /// One-based column number (in characters).
        col: usize,
    },
    /// The parser expected a token of one kind but found another.
    ///
    /// Fields: `expected`, `found`, `offset`, `line`, `col`.
    UnexpectedToken {
        /// Human-readable description of the expected token.
        expected: String,
        /// Human-readable description of the token actually found.
        found: String,
        /// Zero-based byte offset.
        offset: usize,
        /// One-based line number.
        line: usize,
        /// One-based column number (in characters).
        col: usize,
    },
    /// The expression nesting depth exceeded [`MAX_RECURSION_DEPTH`].
    ///
    /// Fields: `depth`, `offset`, `line`, `col`.
    RecursionLimitExceeded {
        /// The depth actually reached (greater than the limit).
        depth: usize,
        /// Zero-based byte offset.
        offset: usize,
        /// One-based line number.
        line: usize,
        /// One-based column number (in characters).
        col: usize,
    },
    /// A variable referenced in the expression was not found in the context.
    ///
    /// Fields: `name`, `offset`, `line`, `col`.
    UndefinedVariable {
        /// Name of the missing variable.
        name: String,
        /// Zero-based byte offset.
        offset: usize,
        /// One-based line number.
        line: usize,
        /// One-based column number (in characters).
        col: usize,
    },
    /// An operation was applied to incompatible types.
    ///
    /// Fields: `expected`, `found`, `offset`, `line`, `col`.
    TypeMismatch {
        /// Description of the expected type.
        expected: String,
        /// Description of the found type.
        found: String,
        /// Zero-based byte offset.
        offset: usize,
        /// One-based line number.
        line: usize,
        /// One-based column number (in characters).
        col: usize,
    },
    /// Division or modulo by zero was attempted.
    ///
    /// Fields: `offset`, `line`, `col`.
    DivisionByZero {
        /// Zero-based byte offset.
        offset: usize,
        /// One-based line number.
        line: usize,
        /// One-based column number (in characters).
        col: usize,
    },
    /// An integer arithmetic operation overflowed `i64`/`u64` bounds.
    ///
    /// Fields: `offset`, `line`, `col`.
    IntegerOverflow {
        /// Zero-based byte offset.
        offset: usize,
        /// One-based line number.
        line: usize,
        /// One-based column number (in characters).
        col: usize,
    },
    /// An index or key lookup fell outside the bounds of a list or map.
    ///
    /// Fields: `offset`, `line`, `col`.
    IndexOutOfBounds {
        /// Zero-based byte offset.
        offset: usize,
        /// One-based line number.
        line: usize,
        /// One-based column number (in characters).
        col: usize,
    },
    /// The evaluation step budget (`max_steps`) was exhausted.
    ///
    /// Fields: `max_steps`.
    StepLimitExceeded {
        /// The configured maximum number of steps.
        max_steps: usize,
    },
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExprError::UnexpectedChar {
                character,
                offset,
                line,
                col,
            } => write!(
                f,
                "unexpected character `{character}` at offset {offset} (line {line}, col {col})"
            ),
            ExprError::UnterminatedString { offset, line, col } => write!(
                f,
                "unterminated string literal at offset {offset} (line {line}, col {col})"
            ),
            ExprError::UnexpectedToken {
                expected,
                found,
                offset,
                line,
                col,
            } => write!(
                f,
                "expected {expected} but found {found} at offset {offset} (line {line}, col {col})"
            ),
            ExprError::RecursionLimitExceeded {
                depth,
                offset,
                line,
                col,
            } => write!(
                f,
                "recursion limit exceeded (depth {depth} > {MAX_RECURSION_DEPTH}) \
                 at offset {offset} (line {line}, col {col})"
            ),
            ExprError::UndefinedVariable { name, .. } => {
                write!(f, "undefined variable `{name}`")
            }
            ExprError::TypeMismatch { expected, found, .. } => {
                write!(f, "type mismatch: expected {expected}, found {found}")
            }
            ExprError::DivisionByZero { .. } => write!(f, "division by zero"),
            ExprError::IntegerOverflow { .. } => write!(f, "integer overflow"),
            ExprError::IndexOutOfBounds { .. } => write!(f, "index out of bounds"),
            ExprError::StepLimitExceeded { max_steps } => {
                write!(f, "step limit exceeded ({max_steps} steps)")
            }
        }
    }
}

impl std::error::Error for ExprError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_position() {
        let err = ExprError::UnexpectedChar {
            character: '@',
            offset: 4,
            line: 1,
            col: 5,
        };
        let s = err.to_string();
        assert!(s.contains("unexpected character"));
        assert!(s.contains("offset 4"));
        assert!(s.contains("line 1"));
        assert!(s.contains("col 5"));

        let e2 = ExprError::UndefinedVariable {
            name: "x".into(),
            offset: 0,
            line: 1,
            col: 1,
        };
        assert_eq!(
            e2.to_string(),
            "undefined variable `x`"
        );

        let e3 = ExprError::StepLimitExceeded { max_steps: 100 };
        assert!(e3.to_string().contains("100"));

        let e4 = ExprError::RecursionLimitExceeded {
            depth: 100,
            offset: 0,
            line: 1,
            col: 1,
        };
        assert!(e4.to_string().contains("recursion limit exceeded"));
        assert_eq!(e4, e4.clone());
        assert!(e4 == e4.clone());
    }
}