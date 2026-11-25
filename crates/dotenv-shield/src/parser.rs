//! Strict `.env` parser.
//!
//! The parser understands the following grammar:
//!
//! * blank lines and `#` comment lines are ignored;
//! * `KEY=VALUE` and `export KEY=VALUE` assignments;
//! * unquoted values, where an inline `#` comment is stripped;
//! * single-quoted values, taken literally;
//! * double-quoted values, supporting `\n`, `\r`, `\t`, `\"`, `\\` escapes and
//!   spanning multiple physical lines.
//!
//! Interpolation of `${VAR}` / `$VAR` references is performed separately by
//! [`resolve_entries`] so that parsing stays a pure, allocation-light pass.

use crate::error::DotenvError;

/// A single `KEY=VALUE` assignment extracted from a `.env` document.
///
/// The `value` field holds the *raw* value after quote processing but **before**
/// interpolation; call [`resolve_entries`] to expand `${VAR}` / `$VAR`
/// references.
///
/// # Examples
///
/// ```
/// use dotenv_shield::parse_dotenv;
///
/// let entries = parse_dotenv("PORT=8080").unwrap();
/// assert_eq!(entries[0].key, "PORT");
/// assert_eq!(entries[0].value, "8080");
/// assert_eq!(entries[0].line, 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvEntry {
    /// The variable name, guaranteed to match `[A-Za-z_][A-Za-z0-9_]*`.
    pub key: String,
    /// The value with quotes removed and escapes expanded.
    pub value: String,
    /// 1-based line number where the assignment starts.
    pub line: usize,
}

/// Parses a `.env` document into an ordered list of [`EnvEntry`].
///
/// Entries are returned in source order, which is the order used by
/// [`resolve_entries`] when expanding references.
///
/// # Errors
///
/// Returns a [`DotenvError`] carrying the 1-based line number when the input
/// contains an invalid key, an unterminated quote, a NUL byte, a dangling
/// escape sequence, or a line that is not a valid assignment.
///
/// # Examples
///
/// ```
/// use dotenv_shield::parse_dotenv;
///
/// let input = concat!(
///     "export DB_HOST=localhost\n",
///     "DB_PORT=5432\n",
///     "DB_NAME='my db'\n",
///     "DB_PASS=\"s3cr\\t\"\n",
/// );
/// let entries = parse_dotenv(input).unwrap();
/// assert_eq!(entries.len(), 4);
/// assert_eq!(entries[0].key, "DB_HOST");
/// assert_eq!(entries[0].value, "localhost");
/// assert_eq!(entries[2].value, "my db");
/// assert_eq!(entries[3].value, "s3cr\t");
/// ```
pub fn parse_dotenv(input: &str) -> Result<Vec<EnvEntry>, DotenvError> {
    let mut entries = Vec::new();
    let mut chars = input.char_indices().peekable();
    let mut line: usize = 1;

    // Reject NUL bytes up front so that every later stage can assume clean text.
    if let Some(offset) = input.find('\0') {
        let nul_line = input[..offset].matches('\n').count() + 1;
        return Err(DotenvError::NulByte { line: nul_line });
    }

    loop {
        // Skip leading whitespace on the current logical line.
        while let Some(&(_, ch)) = chars.peek() {
            if ch == ' ' || ch == '\t' || ch == '\r' {
                chars.next();
            } else {
                break;
            }
        }

        let Some(&(_, first)) = chars.peek() else {
            break;
        };

        // Blank line or comment line.
        if first == '\n' {
            chars.next();
            line += 1;
            continue;
        }
        if first == '#' {
            for (_, ch) in chars.by_ref() {
                if ch == '\n' {
                    line += 1;
                    break;
                }
            }
            continue;
        }

        let entry_line = line;

        // Read the key, honouring the optional `export ` prefix.
        let mut key = read_identifier(&mut chars);

        // Skip horizontal whitespace between key and `=`.
        skip_horizontal_whitespace(&mut chars);

        // `export KEY=VALUE`: the first identifier was `export`, so the real key
        // starts after the following whitespace.
        if key == "export" {
            key = read_identifier(&mut chars);
            skip_horizontal_whitespace(&mut chars);
        }

        let Some((_, sep)) = chars.next() else {
            return Err(DotenvError::MissingAssignment { line: entry_line });
        };
        if sep != '=' {
            return Err(DotenvError::MissingAssignment { line: entry_line });
        }

        if !is_valid_key(&key) {
            return Err(DotenvError::InvalidKey {
                line: entry_line,
                key,
            });
        }

        // Skip horizontal whitespace before the value.
        while let Some(&(_, ch)) = chars.peek() {
            if ch == ' ' || ch == '\t' || ch == '\r' {
                chars.next();
            } else {
                break;
            }
        }

        let value = match chars.peek() {
            Some(&(_, '\'')) => {
                chars.next();
                read_single_quoted(&mut chars, entry_line)?
            }
            Some(&(_, '"')) => {
                chars.next();
                read_double_quoted(&mut chars, entry_line, &mut line)?
            }
            _ => read_unquoted(&mut chars, &mut line),
        };

        entries.push(EnvEntry {
            key,
            value,
            line: entry_line,
        });

        // Consume the rest of the physical line (trailing comment or newline).
        for (_, ch) in chars.by_ref() {
            if ch == '\n' {
                line += 1;
                break;
            }
        }
    }

    Ok(entries)
}

/// Expands `${VAR}` and `$VAR` references in every entry, in source order.
///
/// A reference is resolved against the entries that appear **before** the entry
/// being processed. Unknown references expand to an empty string, mirroring the
/// behaviour of common `.env` loaders. `$$` is treated as a literal `$`.
///
/// # Examples
///
/// ```
/// use dotenv_shield::{parse_dotenv, resolve_entries};
///
/// let mut entries = parse_dotenv("HOST=example.com\nURL=https://${HOST}/api\n").unwrap();
/// resolve_entries(&mut entries);
/// assert_eq!(entries[1].value, "https://example.com/api");
/// ```
pub fn resolve_entries(entries: &mut [EnvEntry]) {
    let mut resolved: Vec<(String, String)> = Vec::with_capacity(entries.len());

    for entry in entries.iter_mut() {
        let expanded = interpolate(&entry.value, &resolved);
        resolved.push((entry.key.clone(), expanded.clone()));
        entry.value = expanded;
    }
}

/// Reads an identifier made of ASCII alphanumerics and `_`.
fn read_identifier<I>(chars: &mut std::iter::Peekable<I>) -> String
where
    I: Iterator<Item = (usize, char)>,
{
    let mut out = String::new();
    while let Some(&(_, ch)) = chars.peek() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
            chars.next();
        } else {
            break;
        }
    }
    out
}

/// Skips spaces, tabs and carriage returns.
fn skip_horizontal_whitespace<I>(chars: &mut std::iter::Peekable<I>)
where
    I: Iterator<Item = (usize, char)>,
{
    while let Some(&(_, ch)) = chars.peek() {
        if ch == ' ' || ch == '\t' || ch == '\r' {
            chars.next();
        } else {
            break;
        }
    }
}

/// Returns `true` when `key` matches `[A-Za-z_][A-Za-z0-9_]*`.
fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Reads a single-quoted value (literal, no escape processing).
fn read_single_quoted<I>(
    chars: &mut std::iter::Peekable<I>,
    entry_line: usize,
) -> Result<String, DotenvError>
where
    I: Iterator<Item = (usize, char)>,
{
    let mut value = String::new();
    for (_, ch) in chars.by_ref() {
        if ch == '\'' {
            return Ok(value);
        }
        value.push(ch);
    }
    Err(DotenvError::UnterminatedQuote {
        line: entry_line,
        quote: '\'',
    })
}

/// Reads a double-quoted value, expanding escapes and allowing newlines.
fn read_double_quoted<I>(
    chars: &mut std::iter::Peekable<I>,
    entry_line: usize,
    line: &mut usize,
) -> Result<String, DotenvError>
where
    I: Iterator<Item = (usize, char)>,
{
    let mut value = String::new();
    // A `for` loop cannot be used here: the escape branch needs to pull the
    // next character from the same iterator.
    #[allow(clippy::while_let_on_iterator)]
    while let Some((_, ch)) = chars.next() {
        match ch {
            '"' => return Ok(value),
            '\\' => match chars.next() {
                Some((_, 'n')) => value.push('\n'),
                Some((_, 'r')) => value.push('\r'),
                Some((_, 't')) => value.push('\t'),
                Some((_, '"')) => value.push('"'),
                Some((_, '\\')) => value.push('\\'),
                Some((_, '$')) => value.push('$'),
                Some((_, '\n')) => {
                    // Line continuation inside a double-quoted value.
                    *line += 1;
                }
                Some((_, other)) => {
                    // Unknown escape: keep the backslash for fidelity.
                    value.push('\\');
                    value.push(other);
                }
                None => return Err(DotenvError::DanglingEscape { line: *line }),
            },
            '\n' => {
                *line += 1;
                value.push('\n');
            }
            other => value.push(other),
        }
    }
    Err(DotenvError::UnterminatedQuote {
        line: entry_line,
        quote: '"',
    })
}

/// Reads an unquoted value, stripping an inline `#` comment.
fn read_unquoted<I>(chars: &mut std::iter::Peekable<I>, line: &mut usize) -> String
where
    I: Iterator<Item = (usize, char)>,
{
    let mut value = String::new();
    while let Some(&(_, ch)) = chars.peek() {
        if ch == '\n' {
            break;
        }
        if ch == '#' {
            // An inline comment starts at `#` preceded by whitespace or at the
            // very beginning of the value.
            let preceded_by_space = value.is_empty()
                || value.ends_with(' ')
                || value.ends_with('\t')
                || value.ends_with('\r');
            if preceded_by_space {
                break;
            }
        }
        value.push(ch);
        chars.next();
    }
    let _ = line;
    value.trim_end_matches([' ', '\t', '\r']).to_string()
}

/// Expands `${VAR}` / `$VAR` references using the already resolved pairs.
fn interpolate(value: &str, resolved: &[(String, String)]) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '$' {
            out.push(ch);
            continue;
        }

        match chars.peek() {
            Some('$') => {
                chars.next();
                out.push('$');
            }
            Some('{') => {
                chars.next();
                let mut name = String::new();
                let mut closed = false;
                for inner in chars.by_ref() {
                    if inner == '}' {
                        closed = true;
                        break;
                    }
                    name.push(inner);
                }
                if closed {
                    out.push_str(lookup(&name, resolved));
                } else {
                    // Unterminated `${`: emit verbatim.
                    out.push_str("${");
                    out.push_str(&name);
                }
            }
            Some(next) if next.is_ascii_alphabetic() || *next == '_' => {
                let mut name = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                out.push_str(lookup(&name, resolved));
            }
            _ => out.push('$'),
        }
    }

    out
}

/// Looks up `name` in the resolved pairs, returning `""` when absent.
fn lookup<'a>(name: &str, resolved: &'a [(String, String)]) -> &'a str {
    resolved
        .iter()
        .rev()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::{parse_dotenv, resolve_entries, EnvEntry};
    use crate::error::DotenvError;

    #[test]
    fn parse_basic_forms() {
        let input = "\
# leading comment
PORT=8080
export HOST=localhost
EMPTY=
SPACED =  value with spaces
INLINE=value # trailing comment
HASH=abc#def
SINGLE='literal $NOT_EXPANDED \\n'
DOUBLE=\"line1\\nline2\\ttabbed \\\"quoted\\\" back\\\\slash\"
MULTI=\"first
second\"
";
        let entries = parse_dotenv(input).expect("valid dotenv");

        let keys: Vec<&str> = entries.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "PORT", "HOST", "EMPTY", "SPACED", "INLINE", "HASH", "SINGLE", "DOUBLE", "MULTI"
            ]
        );

        assert_eq!(entries[0].value, "8080");
        assert_eq!(entries[1].value, "localhost");
        assert_eq!(entries[2].value, "");
        assert_eq!(entries[3].value, "value with spaces");
        assert_eq!(entries[4].value, "value");
        assert_eq!(entries[5].value, "abc#def");
        assert_eq!(entries[6].value, "literal $NOT_EXPANDED \\n");
        assert_eq!(
            entries[7].value,
            "line1\nline2\ttabbed \"quoted\" back\\slash"
        );
        assert_eq!(entries[8].value, "first\nsecond");
        assert_eq!(entries[8].line, 10);
    }

    #[test]
    fn parse_rejects_with_line() {
        let err = parse_dotenv("GOOD=1\n1BAD=2\n").unwrap_err();
        assert!(matches!(err, DotenvError::InvalidKey { line: 2, .. }));
        assert_eq!(err.line(), 2);

        let err = parse_dotenv("A=1\nB='unterminated\n").unwrap_err();
        assert!(matches!(
            err,
            DotenvError::UnterminatedQuote { line: 2, quote: '\'' }
        ));

        let err = parse_dotenv("A=1\nB=\"unterminated\n").unwrap_err();
        assert!(matches!(
            err,
            DotenvError::UnterminatedQuote { line: 2, quote: '"' }
        ));

        let err = parse_dotenv("A=1\nB=va\0lue\n").unwrap_err();
        assert!(matches!(err, DotenvError::NulByte { line: 2 }));

        let err = parse_dotenv("A=1\nJUSTAWORD\n").unwrap_err();
        assert!(matches!(err, DotenvError::MissingAssignment { line: 2 }));

        let err = parse_dotenv("A=\"dangling\\").unwrap_err();
        assert!(matches!(err, DotenvError::DanglingEscape { line: 1 }));
    }

    #[test]
    fn resolve_interpolation() {
        let input = "\
BASE=https://example.com
API=${BASE}/v1
TOKEN=abc
AUTH=Bearer $TOKEN
LITERAL=$$notavar
UNKNOWN=${MISSING}tail
";
        let mut entries = parse_dotenv(input).expect("valid dotenv");
        resolve_entries(&mut entries);

        assert_eq!(entries[1].value, "https://example.com/v1");
        assert_eq!(entries[3].value, "Bearer abc");
        assert_eq!(entries[4].value, "$notavar");
        assert_eq!(entries[5].value, "tail");
    }

    #[test]
    fn interpolation_uses_previous_entries_only() {
        let mut entries = vec![
            EnvEntry {
                key: "A".to_string(),
                value: "$B".to_string(),
                line: 1,
            },
            EnvEntry {
                key: "B".to_string(),
                value: "late".to_string(),
                line: 2,
            },
        ];
        resolve_entries(&mut entries);
        assert_eq!(entries[0].value, "");
        assert_eq!(entries[1].value, "late");
    }

    #[test]
    fn empty_input_is_ok() {
        assert!(parse_dotenv("").unwrap().is_empty());
        assert!(parse_dotenv("\n\n# only comments\n").unwrap().is_empty());
    }
}
