//! `glob_ignore` — a complete path filtering engine.
//!
//! The crate provides two complementary building blocks:
//!
//! * [`GlobPattern`] — a compiled glob pattern (POSIX/Windows flavoured) that
//!   can be matched against a `/`-separated path in guaranteed `O(N * M)` time.
//! * [`IgnoreMatcher`] — a `.gitignore`-compatible rule engine answering the
//!   question *"is this path ignored?"*.
//!
//! The crate is `std`-only, has **zero dependencies**, forbids `unsafe` code and
//! denies missing documentation.
//!
//! # Quick start
//!
//! ```
//! use glob_ignore::GlobPattern;
//!
//! let pattern = GlobPattern::parse("src/**/*.rs").unwrap();
//! assert!(pattern.matches("src/main.rs"));
//! assert!(pattern.matches("src/a/b/c/lib.rs"));
//! assert!(!pattern.matches("src/main.py"));
//! ```
//!
//! ```
//! use glob_ignore::IgnoreMatcher;
//!
//! let matcher = IgnoreMatcher::from_gitignore(
//!     "# build artefacts\n\
//!      /target/\n\
//!      *.log\n\
//!      !keep.log\n",
//! )
//! .unwrap();
//!
//! assert!(matcher.is_ignored("target", true));
//! assert!(matcher.is_ignored("logs/app.log", false));
//! assert!(!matcher.is_ignored("logs/keep.log", false));
//! assert!(!matcher.is_ignored("src/main.rs", false));
//! ```
//!
//! # Complexity guarantee
//!
//! Matching never recurses and never backtracks exponentially. A pattern such as
//! `a*a*a*a*a*a*a*a*b` matched against a long run of `a` characters completes in
//! polynomial time, which makes the engine immune to classic ReDoS payloads.
//!
//! ```
//! use glob_ignore::GlobPattern;
//!
//! let pattern = GlobPattern::parse("a*a*a*a*a*a*a*a*b").unwrap();
//! let subject = "a".repeat(2_000);
//! assert!(!pattern.matches(&subject));
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(missing_debug_implementations)]
#![deny(rust_2018_idioms)]
#![warn(clippy::all)]

mod error;
mod glob;
mod ignore;

pub use crate::error::{GlobError, GlobErrorKind};
pub use crate::glob::GlobPattern;
pub use crate::ignore::IgnoreMatcher;

#[cfg(test)]
mod tests {
    use super::{GlobError, GlobPattern, IgnoreMatcher};

    /// Feeds a wide range of malformed and exotic inputs to every public entry
    /// point and asserts that nothing panics.
    #[test]
    fn no_panic_on_malformed_input() {
        let patterns = [
            "",
            "*",
            "**",
            "***",
            "a**b",
            "[",
            "[]",
            "[!]",
            "[^]",
            "[a-",
            "[z-a]",
            "[a-z",
            "\\",
            "a\\",
            "\\\\",
            "?",
            "?/?",
            "**/**/**",
            "/**/",
            "a/**/b/**/c",
            "é*ü?ñ",
            "🦀/**/🦀",
            "\u{0}",
            "a\u{0}b",
            "[🦀-🦀]",
            "[]]",
            "[!]]",
            "a/b/c/d/e/f/g",
        ];

        for pattern in patterns {
            if let Ok(compiled) = GlobPattern::parse(pattern) {
                let subjects = [
                    "",
                    "/",
                    "a",
                    "a/b",
                    "a/b/c",
                    "é/ü/ñ",
                    "🦀/🦀",
                    "\u{0}",
                    "a\\b\\c",
                    "a//b",
                    "///",
                    "./a/./b",
                ];
                for subject in subjects {
                    let _ = compiled.matches(subject);
                }
            }
        }
    }

    /// Feeds malformed `.gitignore` documents to the matcher.
    #[test]
    fn no_panic_on_malformed_gitignore() {
        let documents = [
            "",
            "\n\n\n",
            "# only a comment\n",
            "!",
            "!!",
            "/",
            "//",
            "\\",
            "\\#not-a-comment\n",
            "trailing   \n",
            "trailing\\ \n",
            "[abc\n",
            "a**b\n",
            "**\n",
            "**/\n",
            "/**\n",
            "dir/\n",
            "!dir/\n",
            "é/ü\n",
            "🦀\n",
            "\u{0}\n",
            "a\r\nb\r\n",
        ];

        for document in documents {
            if let Ok(matcher) = IgnoreMatcher::from_gitignore(document) {
                let paths = ["", "/", "a", "a/b", "dir", "dir/file", "é/ü", "🦀", "\u{0}"];
                for path in paths {
                    let _ = matcher.is_ignored(path, false);
                    let _ = matcher.is_ignored(path, true);
                }
            }
        }
    }

    /// Ensures the error type is reachable from the crate root.
    #[test]
    fn no_panic_error_display() {
        let err: GlobError = GlobPattern::parse("[abc").unwrap_err();
        let rendered = err.to_string();
        assert!(!rendered.is_empty());
    }

    /// A pathological pattern must not blow up the runtime.
    #[test]
    fn no_panic_redos_payload() {
        let pattern = GlobPattern::parse("a*a*a*a*a*a*a*a*a*a*b").unwrap();
        let subject = "a".repeat(5_000);
        assert!(!pattern.matches(&subject));
    }
}
