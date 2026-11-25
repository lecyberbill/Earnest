# glob_ignore

A complete path filtering engine for Rust: `.gitignore`-compatible rules and
POSIX/Windows glob patterns, with **guaranteed polynomial-time matching**.

* `std` only — **zero dependencies**
* `#![forbid(unsafe_code)]` and `#![deny(missing_docs)]`
* Fully iterative matcher: no recursion, no exponential backtracking
* No panics on malformed or non-ASCII input

## Installation

```toml
[dependencies]
glob_ignore = "0.1"
```

## Glob patterns

```rust
use glob_ignore::GlobPattern;

let pattern = GlobPattern::parse("src/**/*.rs").unwrap();
assert!(pattern.matches("src/main.rs"));
assert!(pattern.matches("src/parser/lexer.rs"));
assert!(!pattern.matches("src/main.rs.bak"));
```

### Supported syntax

| Token    | Meaning                                                     |
|----------|-------------------------------------------------------------|
| `*`      | Any sequence of characters except `/` (possibly empty).     |
| `**`     | Zero or more whole path segments (`**/foo`, `foo/**`, `a/**/b`). |
| `?`      | Exactly one character except `/`.                           |
| `[abc]`  | One character from the set.                                 |
| `[a-z]`  | One character from the range.                               |
| `[!a-z]` | One character **not** in the set (`[^a-z]` is equivalent).  |
| `\x`     | The literal character `x`.                                  |

Windows separators (`\`) in the *subject path* are normalised to `/` before
matching. Inside a *pattern*, `\` is the escape character, so a literal
backslash must be written `\\`.

### Complexity

The matcher uses a two-pointer scan with a single remembered star position, so
matching runs in `O(N * M)` where `N` is the pattern length and `M` the subject
length. Pathological patterns such as `a*a*a*a*a*a*b` cannot trigger
exponential backtracking (ReDoS).

## `.gitignore` rules

```rust
use glob_ignore::IgnoreMatcher;

let matcher = IgnoreMatcher::from_gitignore(
    "# build artefacts\n\
     /target/\n\
     *.log\n\
     !important.log\n",
)
.unwrap();

assert!(matcher.is_ignored("target", true));
assert!(matcher.is_ignored("logs/app.log", false));
assert!(!matcher.is_ignored("important.log", false));
```

### Rule semantics

* Blank lines are ignored.
* Lines starting with `#` are comments (escape a leading `#` with `\#`).
* Trailing unescaped whitespace is stripped; escape a trailing space with `\ `
  to keep it.
* A leading `!` negates the rule (re-inclusion).
* A leading `/` anchors the pattern to the root of the tree.
* A trailing `/` restricts the rule to directories (`is_dir == true`).
* A pattern without any `/` matches the basename at any depth.
* The **last matching rule wins**.
* A re-inclusion (`!`) cannot resurrect a path whose parent directory is
  excluded.

## Errors

Parsing returns `Result<_, GlobError>`. `GlobError` implements
`std::error::Error` and carries the offending pattern, the byte offset of the
problem and a `GlobErrorKind` describing the syntax error.

## License

Licensed under either of Apache-2.0 or MIT at your option.
