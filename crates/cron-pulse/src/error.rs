//! Error type returned by every fallible operation of the crate.

use std::fmt;

/// Detailed error returned when a cron expression cannot be parsed or validated.
///
/// The type is intentionally small, `Clone`, `PartialEq` and `Eq` so that it can
/// be compared in tests and stored in application state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CronError {
    /// The expression was empty or contained only whitespace.
    EmptyExpression,
    /// The expression does not have exactly five whitespace-separated fields.
    InvalidFieldCount {
        /// Number of fields that were actually found.
        found: usize,
    },
    /// A macro (`@daily`, `@weekly`, ...) was not recognised.
    UnknownMacro {
        /// The offending macro token, as written by the user.
        name: String,
    },
    /// A field could not be parsed.
    InvalidField {
        /// Name of the field (`minute`, `hour`, `day_of_month`, `month`, `day_of_week`).
        field: &'static str,
        /// The raw text of the field.
        value: String,
        /// Human readable explanation of the problem.
        reason: String,
    },
    /// A list item was empty (e.g. `1,,2` or a trailing comma).
    EmptyItem {
        /// Name of the field.
        field: &'static str,
    },
    /// A numeric value is outside the allowed range of its field.
    ValueOutOfRange {
        /// Name of the field.
        field: &'static str,
        /// The offending value, as written by the user.
        value: String,
        /// Inclusive lower bound of the field.
        min: u32,
        /// Inclusive upper bound of the field.
        max: u32,
    },
    /// A step (`/n`) was not a valid positive integer.
    InvalidStep {
        /// Name of the field.
        field: &'static str,
        /// The offending step text.
        value: String,
    },
    /// A step (`/n`) was zero, which would never terminate.
    ZeroStep {
        /// Name of the field.
        field: &'static str,
    },
    /// A range was written with its lower bound greater than its upper bound.
    ReversedRange {
        /// Name of the field.
        field: &'static str,
        /// Lower bound as written.
        start: u32,
        /// Upper bound as written.
        end: u32,
    },
    /// An alphabetic token is not a known alias for the field.
    UnknownAlias {
        /// Name of the field.
        field: &'static str,
        /// The offending token.
        token: String,
    },
    /// The expression is syntactically valid but can never fire (e.g. `* * 31 2 *`).
    ImpossibleDate {
        /// Explanation of why the date can never occur.
        reason: String,
    },
}

impl fmt::Display for CronError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CronError::EmptyExpression => write!(f, "cron expression is empty"),
            CronError::InvalidFieldCount { found } => write!(
                f,
                "cron expression must have exactly 5 fields, found {}",
                found
            ),
            CronError::UnknownMacro { name } => write!(f, "unknown cron macro `{}`", name),
            CronError::InvalidField {
                field,
                value,
                reason,
            } => write!(f, "invalid `{}` field `{}`: {}", field, value, reason),
            CronError::EmptyItem { field } => {
                write!(f, "empty list item in `{}` field", field)
            }
            CronError::ValueOutOfRange {
                field,
                value,
                min,
                max,
            } => write!(
                f,
                "value {} is out of range for `{}` field (allowed {}..={})",
                value, field, min, max
            ),
            CronError::InvalidStep { field, value } => {
                write!(f, "invalid step `{}` in `{}` field", value, field)
            }
            CronError::ZeroStep { field } => {
                write!(f, "step value 0 is not allowed in `{}` field", field)
            }
            CronError::ReversedRange { field, start, end } => write!(
                f,
                "reversed range {}-{} in `{}` field",
                start, end, field
            ),
            CronError::UnknownAlias { field, token } => {
                write!(f, "unknown alias `{}` in `{}` field", token, field)
            }
            CronError::ImpossibleDate { reason } => {
                write!(f, "cron expression can never fire: {}", reason)
            }
        }
    }
}

impl std::error::Error for CronError {}
