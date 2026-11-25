//! # `dotenv_shield`
//!
//! A strict `.env` parser combined with a high-precision secret detector and
//! redactor. The crate is written in pure `std` Rust, has **zero external
//! dependencies**, forbids `unsafe` code and denies missing documentation.
//!
//! ## Quick start
//!
//! ```
//! use dotenv_shield::{parse_dotenv, resolve_entries};
//!
//! let input = "PORT=8080\nexport HOST=localhost\n";
//! let mut entries = parse_dotenv(input).expect("valid dotenv");
//! resolve_entries(&mut entries);
//! assert_eq!(entries[0].key, "PORT");
//! assert_eq!(entries[0].value, "8080");
//! assert_eq!(entries[1].value, "localhost");
//! ```
//!
//! ## Secret detection
//!
//! ```
//! use dotenv_shield::{scan_secrets, SecretKind};
//!
//! let findings = scan_secrets("token = \"sk-abcdefghijklmnopqrstuvwxyz012345\"");
//! assert!(findings.iter().any(|f| f.kind == SecretKind::OpenAi));
//! ```
//!
//! ## Redaction
//!
//! ```
//! use dotenv_shield::redact_secrets;
//!
//! let redacted = redact_secrets("url = \"https://user:hunter2@example.com/path\"");
//! assert!(redacted.contains("[REDACTED:url_credentials]"));
//! assert!(redacted.contains("example.com/path"));
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(missing_debug_implementations)]
#![warn(rust_2018_idioms)]

mod entropy;
mod error;
mod parser;
mod redact;
mod scan;

pub use entropy::shannon_entropy;
pub use error::DotenvError;
pub use parser::{parse_dotenv, resolve_entries, EnvEntry};
pub use redact::{mask_dotenv_template, redact_secrets};
pub use scan::{scan_secrets, SecretFinding, SecretKind};
