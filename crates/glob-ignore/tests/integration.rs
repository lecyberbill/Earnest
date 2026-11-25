//! Integration tests exercising the public API end to end.

use std::time::Instant;

use glob_ignore::{GlobError, GlobPattern, IgnoreMatcher};

#[test]
fn glob_matches_nested_paths() {
    let pattern = GlobPattern::parse("src/**/*.rs").unwrap();
    assert!(pattern.matches("src/main.rs"));
    assert!(pattern.matches("src/a/b/c.rs"));
    assert!(!pattern.matches("src/main.rs.bak"));
    assert!(!pattern.matches("tests/main.rs"));
}

#[test]
fn glob_windows_separators_are_normalised() {
    // A `\` that precedes a non-metacharacter is a Windows path separator and
    // is normalised to `/`. Here `\p` and `\l` are separators.
    let pattern = GlobPattern::parse(r"src\parser\lexer.rs").unwrap();
    assert!(pattern.matches(r"src\parser\lexer.rs"));
    assert!(pattern.matches("src/parser/lexer.rs"));
    assert!(!pattern.matches("src/other/lexer.rs"));

    // A `\` that precedes a glob metacharacter is an escape, so `\*` is a
    // literal `*` and `\**` is a literal `*` followed by a wildcard.
    let escaped = GlobPattern::parse(r"a\*b").unwrap();
    assert!(escaped.matches("a*b"));
    assert!(!escaped.matches("axb"));
}

#[test]
fn glob_character_classes() {
    let pattern = GlobPattern::parse("file[0-9].txt").unwrap();
    assert!(pattern.matches("file7.txt"));
    assert!(!pattern.matches("filex.txt"));

    let negated = GlobPattern::parse("file[!0-9].txt").unwrap();
    assert!(negated.matches("filex.txt"));
    assert!(!negated.matches("file7.txt"));
}

#[test]
fn glob_errors_are_reported() {
    assert!(matches!(
        GlobPattern::parse("[abc"),
        Err(GlobError { .. })
    ));
    assert!(GlobPattern::parse("a\\").is_err());
}

#[test]
fn no_redos_on_pathological_pattern() {
    let pattern = GlobPattern::parse("a*a*a*a*a*a*a*a*a*a*b").unwrap();
    let subject = "a".repeat(20_000);
    let start = Instant::now();
    assert!(!pattern.matches(&subject));
    assert!(start.elapsed().as_secs() < 5, "matching took too long");
}

#[test]
fn no_redos_on_double_star_pattern() {
    let pattern = GlobPattern::parse("**/**/**/**/**/**/x").unwrap();
    let subject = "a/".repeat(5_000) + "b";
    let start = Instant::now();
    assert!(!pattern.matches(&subject));
    assert!(start.elapsed().as_secs() < 5, "matching took too long");
}

#[test]
fn gitignore_end_to_end() {
    let matcher = IgnoreMatcher::from_gitignore(
        "# comment\n\
         \n\
         /target/\n\
         *.log\n\
         !important.log\n\
         build/\n",
    )
    .unwrap();

    assert!(matcher.is_ignored("target", true));
    assert!(!matcher.is_ignored("target", false));
    assert!(matcher.is_ignored("logs/app.log", false));
    assert!(!matcher.is_ignored("important.log", false));
    assert!(matcher.is_ignored("crates/build", true));
    assert!(!matcher.is_ignored("src/main.rs", false));
}

#[test]
fn gitignore_last_match_wins() {
    let matcher = IgnoreMatcher::from_gitignore("*.log\n!keep.log\n").unwrap();
    assert!(matcher.is_ignored("a.log", false));
    assert!(!matcher.is_ignored("keep.log", false));
}

#[test]
fn gitignore_cannot_reinclude_under_excluded_parent() {
    let matcher = IgnoreMatcher::from_gitignore("build/\n!build/keep.txt\n").unwrap();
    assert!(matcher.is_ignored("build/keep.txt", false));
}

#[test]
fn no_panic_on_unicode_and_malformed_input() {
    for pattern in ["", "***", "[]", "[!]", "[z-a]", "a**b", "\\", "é*ü", "???"] {
        let _ = GlobPattern::parse(pattern);
    }
    for content in ["", "\n\n\n", "#only comment", "!", "/", "\\", "é/ü\n"] {
        let _ = IgnoreMatcher::from_gitignore(content);
    }
    let matcher = IgnoreMatcher::from_gitignore("é*\n").unwrap();
    assert!(matcher.is_ignored("été.txt", false));
}
