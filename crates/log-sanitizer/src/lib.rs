//! # log-sanitizer
//!
//! Pure-Rust detector and redactor for secrets (`API_KEY`, Bearer tokens,
//! PEM private keys, `.env` assignments, URL credentials) before sending
//! contexts or logs to external LLMs or the UI. Zero external dependencies.

pub mod sanitizer;

pub use sanitizer::{Redaction, SanitizeReport, SecretKind, contains_secrets, sanitize_report, sanitize_text};
