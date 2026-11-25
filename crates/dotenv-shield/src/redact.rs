//! Safe redaction of detected secrets.
//!
//! [`redact_secrets`] replaces every secret reported by
//! [`scan_secrets`](crate::scan_secrets) with a `[REDACTED:<kind>]` marker while
//! leaving the surrounding structure (JSON, YAML, TOML, URLs, source code)
//! untouched.
//!
//! [`mask_dotenv_template`] turns a real `.env` file into a committable
//! `.env.example` template: comments and non-sensitive keys are preserved,
//! sensitive values are replaced by `<YOUR_KEY_NAME>`.

use crate::error::DotenvError;
use crate::parser::parse_dotenv;
use crate::scan::{scan_secrets, SecretKind};

/// Returns the marker inserted in place of a secret of the given kind.
fn marker(kind: SecretKind) -> String {
    let mut out = String::with_capacity(kind.as_str().len() + 11);
    out.push_str("[REDACTED:");
    out.push_str(kind.as_str());
    out.push(']');
    out
}

/// Replaces every detected secret in `text` with a `[REDACTED:<kind>]` marker.
///
/// The function never alters the surrounding syntax: quotes, commas, colons,
/// brackets and URL structure are preserved byte for byte. Only the secret
/// spans reported by [`scan_secrets`](crate::scan_secrets) are rewritten.
///
/// # Examples
///
/// ```
/// use dotenv_shield::redact_secrets;
///
/// let json = r#"{"api_key": "a8Fk2LmQ9zXr4TbV7nPw"}"#;
/// let redacted = redact_secrets(json);
/// assert_eq!(redacted, r#"{"api_key": "[REDACTED:generic_high_entropy]"}"#);
/// ```
///
/// URLs keep their shape, only the password is masked:
///
/// ```
/// use dotenv_shield::redact_secrets;
///
/// let password = "s3cr3tP4ssw0rd";
/// let scheme = "postgres";
/// let sep = "://";
/// let user = "admin";
/// let colon = ":";
/// let at = "@";
/// let host = "db.internal:5432/app";
/// let url = format!("{scheme}{sep}{user}{colon}{password}{at}{host}");
///
/// let redacted = redact_secrets(&url);
/// assert!(redacted.contains("[REDACTED:url_credentials]"));
/// assert!(redacted.contains("db.internal:5432/app"));
/// ```
#[must_use]
pub fn redact_secrets(text: &str) -> String {
    let findings = scan_secrets(text);
    if findings.is_empty() {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;

    for finding in findings {
        if finding.start < cursor || finding.end > text.len() {
            continue;
        }
        if let Some(before) = text.get(cursor..finding.start) {
            out.push_str(before);
        }
        out.push_str(&marker(finding.kind));
        cursor = finding.end;
    }

    if let Some(tail) = text.get(cursor..) {
        out.push_str(tail);
    }

    out
}

/// Builds a `.env.example` template from a real `.env` document.
///
/// Comments, blank lines and the original ordering are preserved. Keys whose
/// value is not sensitive keep their value (`PORT=8080`, `RUST_LOG=info`),
/// while sensitive values are replaced by `<YOUR_KEY_NAME>`.
///
/// # Errors
///
/// Propagates any [`DotenvError`] returned by
/// [`parse_dotenv`](crate::parse_dotenv), including the 1-based line number of
/// the offending line.
///
/// # Examples
///
/// ```
/// use dotenv_shield::mask_dotenv_template;
///
/// let input = concat!(
///     "# service configuration\n",
///     "PORT=8080\n",
///     "RUST_LOG=info\n",
///     "API_KEY=a8Fk2LmQ9zXr4TbV7nPw\n",
/// );
/// let template = mask_dotenv_template(input).unwrap();
/// assert!(template.contains("# service configuration"));
/// assert!(template.contains("PORT=8080"));
/// assert!(template.contains("RUST_LOG=info"));
/// assert!(template.contains("API_KEY=<YOUR_API_KEY>"));
/// ```
pub fn mask_dotenv_template(input: &str) -> Result<String, DotenvError> {
    let entries = parse_dotenv(input)?;
    let mut out = String::with_capacity(input.len());

    for raw_line in input.split('\n') {
        let trimmed = raw_line.trim_start();
        let is_comment = trimmed.starts_with('#');
        let is_blank = trimmed.is_empty();

        if is_comment || is_blank {
            out.push_str(raw_line);
            out.push('\n');
            continue;
        }

        let key = match key_of_line(trimmed) {
            Some(key) => key,
            None => {
                out.push_str(raw_line);
                out.push('\n');
                continue;
            }
        };

        let entry = entries.iter().find(|entry| entry.key == key);
        let sensitive = match entry {
            Some(entry) => is_sensitive(&entry.key, &entry.value),
            None => true,
        };

        if sensitive {
            let prefix = if trimmed.starts_with("export ") {
                "export "
            } else {
                ""
            };
            out.push_str(prefix);
            out.push_str(&key);
            out.push('=');
            out.push_str(&placeholder(&key));
            out.push('\n');
        } else {
            out.push_str(raw_line);
            out.push('\n');
        }
    }

    // `split('\n')` yields a trailing empty segment for inputs ending with a
    // newline; drop the extra newline we just appended in that case.
    if !input.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }

    Ok(out)
}

/// Extracts the key from a trimmed `KEY=VALUE` or `export KEY=VALUE` line.
fn key_of_line(line: &str) -> Option<String> {
    let rest = line.strip_prefix("export ").unwrap_or(line);
    let sep = rest.find('=')?;
    let key = rest.get(..sep)?.trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

/// Returns `true` when the value of `key` must be masked in a template.
fn is_sensitive(key: &str, value: &str) -> bool {
    if value.is_empty() {
        return false;
    }

    let lower_key = key.to_ascii_lowercase();
    const KEY_MARKERS: &[&str] = &[
        "key",
        "secret",
        "token",
        "password",
        "passwd",
        "pwd",
        "credential",
        "private",
        "auth",
        "signature",
        "salt",
        "dsn",
        "connection",
    ];
    if KEY_MARKERS.iter().any(|marker| lower_key.contains(marker)) {
        return true;
    }

    // A value that the scanner itself flags is sensitive regardless of its key.
    !scan_secrets(value).is_empty()
}

/// Builds the `<YOUR_KEY_NAME>` placeholder for `key`.
fn placeholder(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 6);
    out.push_str("<YOUR_");
    for ch in key.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_uppercase());
        } else {
            out.push('_');
        }
    }
    out.push('>');
    out
}

#[cfg(test)]
mod tests {
    use super::{mask_dotenv_template, redact_secrets};
    use crate::scan::SecretKind;

    /// Builds a synthetic token so that the test sources never contain a
    /// literal credential.
    fn token(prefix: &str, filler: &str, repeat: usize) -> String {
        let mut out = String::from(prefix);
        for _ in 0..repeat {
            out.push_str(filler);
        }
        out
    }

    #[test]
    fn redact_preserves_structure() {
        let secret = token("sk-", "aB3xY9zQ2wE4rT6yU8iO", 2);

        let json = format!("{{\"api_key\": \"{secret}\", \"port\": 8080}}");
        let redacted = redact_secrets(&json);
        assert!(redacted.starts_with("{\"api_key\": \"[REDACTED:"));
        assert!(redacted.ends_with("\", \"port\": 8080}"));
        assert!(!redacted.contains(&secret));

        let yaml = format!("api_key: {secret}\nport: 8080\n");
        let redacted = redact_secrets(&yaml);
        assert!(redacted.starts_with("api_key: [REDACTED:"));
        assert!(redacted.ends_with("\nport: 8080\n"));

        let toml = format!("api_key = \"{secret}\"\nport = 8080\n");
        let redacted = redact_secrets(&toml);
        assert!(redacted.starts_with("api_key = \"[REDACTED:"));
        assert!(redacted.ends_with("\"\nport = 8080\n"));

        let password = "s3cr3tP4ssw0rd";
        let scheme = "postgres";
        let sep = "://";
        let user = "admin";
        let colon = ":";
        let at = "@";
        let host = "db.internal:5432/app";
        let url = format!("{scheme}{sep}{user}{colon}{password}{at}{host}");
        let redacted = redact_secrets(&url);
        assert_eq!(
            redacted,
            format!("{scheme}{sep}{user}{colon}[REDACTED:url_credentials]{at}{host}")
        );
        assert!(redacted.contains("db.internal:5432/app"));

        // Text without any secret is returned unchanged.
        let clean = "PORT=8080\nRUST_LOG=info\n";
        assert_eq!(redact_secrets(clean), clean);
    }

    #[test]
    fn mask_dotenv_template_output() {
        let secret = token("sk-", "aB3xY9zQ2wE4rT6yU8iO", 2);
        let input = format!(
            "# service configuration\n\
             PORT=8080\n\
             RUST_LOG=info\n\
             API_KEY={secret}\n\
             export DB_PASSWORD=hunter2hunter2\n\
             \n\
             # trailing comment\n"
        );

        let template = mask_dotenv_template(&input).expect("valid dotenv");
        assert!(template.contains("# service configuration"));
        assert!(template.contains("PORT=8080"));
        assert!(template.contains("RUST_LOG=info"));
        assert!(template.contains("API_KEY=<YOUR_API_KEY>"));
        assert!(template.contains("export DB_PASSWORD=<YOUR_DB_PASSWORD>"));
        assert!(template.contains("# trailing comment"));
        assert!(!template.contains(&secret));
        assert!(!template.contains("hunter2hunter2"));

        // The generated template must itself be a valid `.env` document.
        assert!(crate::parse_dotenv(&template).is_ok());

        // Errors are propagated with their line number.
        let err = mask_dotenv_template("A=1\n1BAD=2\n").unwrap_err();
        assert_eq!(err.line(), 2);
    }

    #[test]
    fn redact_keeps_kind_markers() {
        let aws = token("AKIA", "Q7RTZ2NPLK4MW8XV", 1);
        let text = format!("aws = {aws}\n");
        let redacted = redact_secrets(&text);
        assert!(redacted.contains(&format!("[REDACTED:{}]", SecretKind::Aws.as_str())));
    }
}
