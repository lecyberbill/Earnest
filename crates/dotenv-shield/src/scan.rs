//! High-precision secret detection.
//!
//! [`scan_secrets`] walks an arbitrary text (source code, JSON, YAML, logs,
//! `.env` files, ...) and reports every span that looks like a credential.
//!
//! Two complementary strategies are used:
//!
//! 1. **Exact signatures** for well-known providers (OpenAI/Anthropic/DeepSeek,
//!    GitHub, AWS, Slack, Google API, JWT, PEM private keys, URLs carrying
//!    credentials).
//! 2. **Generic high-entropy assignments** for `api_key`, `secret`, `token` and
//!    `password` style keys whose value has a Shannon entropy of at least
//!    `3.8` bits per character over at least `20` characters.
//!
//! Obvious placeholders (`YOUR_API_KEY`, `example`, `changeme`, `<token>`,
//! `***`, bare UUIDs, file paths and isolated SHA-256 hex digests) are filtered
//! out so that the detector stays usable in CI.

use crate::entropy::shannon_entropy;

/// Minimum Shannon entropy (bits per character) for the generic heuristic.
const MIN_GENERIC_ENTROPY: f64 = 3.8;
/// Minimum value length for the generic heuristic.
const MIN_GENERIC_LEN: usize = 20;

/// The kind of credential that was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretKind {
    /// An OpenAI, Anthropic or DeepSeek style `sk-...` API key.
    OpenAi,
    /// A GitHub personal access token (`ghp_...` or `github_pat_...`).
    GitHub,
    /// An AWS access key id (`AKIA...`).
    Aws,
    /// A Slack token (`xox[baprs]-...`).
    Slack,
    /// A Google API key (`AIza...`).
    GoogleApi,
    /// A JSON Web Token (three Base64URL segments).
    Jwt,
    /// A PEM encoded private key block.
    PemPrivateKey,
    /// Credentials embedded in a URL (`scheme://user:password@host`).
    UrlCredentials,
    /// A generic high-entropy value assigned to a secret-looking key.
    GenericHighEntropy,
}

impl SecretKind {
    /// Returns the stable, lowercase identifier used in redaction markers.
    ///
    /// ```
    /// use dotenv_shield::SecretKind;
    ///
    /// assert_eq!(SecretKind::Aws.as_str(), "aws");
    /// assert_eq!(SecretKind::UrlCredentials.as_str(), "url_credentials");
    /// ```
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::GitHub => "github",
            Self::Aws => "aws",
            Self::Slack => "slack",
            Self::GoogleApi => "google_api",
            Self::Jwt => "jwt",
            Self::PemPrivateKey => "pem_private_key",
            Self::UrlCredentials => "url_credentials",
            Self::GenericHighEntropy => "generic_high_entropy",
        }
    }
}

/// A single secret occurrence found in a text.
///
/// `start` and `end` are **byte offsets** into the scanned text, so
/// `&text[start..end]` is always a valid slice boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretFinding {
    /// The kind of credential detected.
    pub kind: SecretKind,
    /// Byte offset of the first byte of the secret.
    pub start: usize,
    /// Byte offset one past the last byte of the secret.
    pub end: usize,
    /// 1-based line number where the secret starts.
    pub line: usize,
}

impl SecretFinding {
    /// Returns the matched secret as a string slice of `text`.
    ///
    /// ```
    /// use dotenv_shield::scan_secrets;
    ///
    /// let text = "API_KEY=a8Fk2LmQ9zXr4TbV7nPw";
    /// let findings = scan_secrets(text);
    /// assert_eq!(findings[0].as_str(text), "a8Fk2LmQ9zXr4TbV7nPw");
    /// ```
    #[must_use]
    pub fn as_str<'a>(&self, text: &'a str) -> &'a str {
        text.get(self.start..self.end).unwrap_or("")
    }
}

/// Scans `text` and returns every detected secret, ordered by position.
///
/// Overlapping matches are resolved by keeping the earliest and longest match.
///
/// # Examples
///
/// ```
/// use dotenv_shield::{scan_secrets, SecretKind};
///
/// let text = "API_KEY=a8Fk2LmQ9zXr4TbV7nPw";
/// let findings = scan_secrets(text);
/// assert_eq!(findings.len(), 1);
/// assert_eq!(findings[0].kind, SecretKind::GenericHighEntropy);
/// ```
#[must_use]
pub fn scan_secrets(text: &str) -> Vec<SecretFinding> {
    let mut findings: Vec<SecretFinding> = Vec::new();

    scan_pem_blocks(text, &mut findings);
    scan_url_credentials(text, &mut findings);
    scan_prefixed_tokens(text, &mut findings);
    scan_aws_keys(text, &mut findings);
    scan_google_keys(text, &mut findings);
    scan_jwts(text, &mut findings);
    scan_generic_assignments(text, &mut findings);

    findings.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    findings.dedup_by(|a, b| a.start == b.start && a.end == b.end);

    // Drop findings fully contained in an earlier, longer finding.
    let mut filtered: Vec<SecretFinding> = Vec::with_capacity(findings.len());
    for finding in findings {
        let contained = filtered
            .last()
            .is_some_and(|prev| finding.start >= prev.start && finding.end <= prev.end);
        if !contained {
            filtered.push(finding);
        }
    }
    filtered
}

/// Returns the 1-based line number of the byte `offset` in `text`.
fn line_of(text: &str, offset: usize) -> usize {
    let clamped = offset.min(text.len());
    text.as_bytes()
        .get(..clamped)
        .map_or(1, |prefix| prefix.iter().filter(|b| **b == b'\n').count() + 1)
}

/// Pushes a finding if the span is a valid char boundary.
fn push_finding(
    text: &str,
    findings: &mut Vec<SecretFinding>,
    kind: SecretKind,
    start: usize,
    end: usize,
) {
    if start >= end
        || end > text.len()
        || !text.is_char_boundary(start)
        || !text.is_char_boundary(end)
    {
        return;
    }
    findings.push(SecretFinding {
        kind,
        start,
        end,
        line: line_of(text, start),
    });
}

/// Returns `true` when the character before `index` does not glue the match to
/// a larger identifier.
fn left_boundary_ok(text: &str, index: usize) -> bool {
    match text.get(..index).and_then(|prefix| prefix.chars().next_back()) {
        None => true,
        Some(c) => !(c.is_ascii_alphanumeric() || c == '_' || c == '-'),
    }
}

/// Detects PEM encoded private key blocks.
fn scan_pem_blocks(text: &str, findings: &mut Vec<SecretFinding>) {
    const BEGIN: &str = "-----BEGIN ";
    const DASHES: &str = "-----";
    const END: &str = "-----END ";

    let mut cursor = 0usize;
    while let Some(rel) = text.get(cursor..).and_then(|rest| rest.find(BEGIN)) {
        let start = cursor + rel;
        let header_end = match text
            .get(start + BEGIN.len()..)
            .and_then(|rest| rest.find(DASHES))
        {
            Some(rel_end) => start + BEGIN.len() + rel_end + DASHES.len(),
            None => break,
        };
        let header = text.get(start..header_end).unwrap_or("");
        if !header.contains("PRIVATE KEY") {
            cursor = header_end;
            continue;
        }
        let body = text.get(header_end..).unwrap_or("");
        let end = match body.find(END) {
            Some(rel_end) => {
                let tail = body.get(rel_end..).unwrap_or("");
                match tail.get(END.len()..).and_then(|rest| rest.find(DASHES)) {
                    Some(rel_close) => header_end + rel_end + END.len() + rel_close + DASHES.len(),
                    None => text.len(),
                }
            }
            None => text.len(),
        };
        push_finding(text, findings, SecretKind::PemPrivateKey, start, end);
        cursor = end.max(start + 1);
    }
}

/// Detects credentials embedded in URLs.
fn scan_url_credentials(text: &str, findings: &mut Vec<SecretFinding>) {
    let mut index = 0usize;

    while index + 3 <= text.len() {
        if !text.is_char_boundary(index) {
            index += 1;
            continue;
        }
        let rest = text.get(index..).unwrap_or("");
        let scheme_len = match rest.find("://") {
            Some(pos) => pos,
            None => break,
        };
        let scheme = rest.get(..scheme_len).unwrap_or("");
        let scheme_ok = !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
            && scheme
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic());
        if !scheme_ok {
            index += 1;
            continue;
        }

        let authority_start = index + scheme_len + 3;
        let authority = text.get(authority_start..).unwrap_or("");
        let authority_end = authority
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '<' || c == '>')
            .unwrap_or(authority.len());
        let authority_slice = authority.get(..authority_end).unwrap_or("");

        if let Some(at) = authority_slice.rfind('@') {
            let userinfo = authority_slice.get(..at).unwrap_or("");
            if let Some(colon) = userinfo.find(':') {
                let password = userinfo.get(colon + 1..).unwrap_or("");
                if !password.is_empty() && !is_placeholder(password) {
                    let start = authority_start + colon + 1;
                    let end = authority_start + at;
                    push_finding(text, findings, SecretKind::UrlCredentials, start, end);
                }
            }
        }

        index = authority_start + authority_end.max(1);
    }
}

/// Detects provider-prefixed tokens.
fn scan_prefixed_tokens(text: &str, findings: &mut Vec<SecretFinding>) {
    const PREFIXES: &[(&str, SecretKind)] = &[
        ("sk-", SecretKind::OpenAi),
        ("ghp_", SecretKind::GitHub),
        ("github_pat_", SecretKind::GitHub),
        ("xoxb-", SecretKind::Slack),
        ("xoxa-", SecretKind::Slack),
        ("xoxp-", SecretKind::Slack),
        ("xoxr-", SecretKind::Slack),
        ("xoxs-", SecretKind::Slack),
    ];

    for (prefix, kind) in PREFIXES {
        let mut cursor = 0usize;
        while let Some(rel) = text.get(cursor..).and_then(|rest| rest.find(prefix)) {
            let start = cursor + rel;
            if !left_boundary_ok(text, start) {
                cursor = start + prefix.len();
                continue;
            }
            let body = text.get(start + prefix.len()..).unwrap_or("");
            let body_len = body
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                .unwrap_or(body.len());
            let token = body.get(..body_len).unwrap_or("");
            let end = start + prefix.len() + body_len;
            if token.len() >= 16 && !is_placeholder(token) {
                push_finding(text, findings, *kind, start, end);
            }
            cursor = end.max(start + 1);
        }
    }
}

/// Detects AWS access key ids.
fn scan_aws_keys(text: &str, findings: &mut Vec<SecretFinding>) {
    const PREFIX: &str = "AKIA";
    const TOTAL: usize = 20;

    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index + TOTAL <= bytes.len() {
        if bytes.get(index..index + PREFIX.len()) == Some(PREFIX.as_bytes()) {
            let candidate = text.get(index..index + TOTAL).unwrap_or("");
            let ok = candidate.len() == TOTAL
                && candidate.get(PREFIX.len()..).is_some_and(|tail| {
                    tail.chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                });
            if ok && left_boundary_ok(text, index) && !is_placeholder(candidate) {
                push_finding(text, findings, SecretKind::Aws, index, index + TOTAL);
                index += TOTAL;
                continue;
            }
        }
        index += 1;
    }
}

/// Detects Google API keys.
fn scan_google_keys(text: &str, findings: &mut Vec<SecretFinding>) {
    const PREFIX: &str = "AIza";
    const TOTAL: usize = 39;

    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index + TOTAL <= bytes.len() {
        if bytes.get(index..index + PREFIX.len()) == Some(PREFIX.as_bytes()) {
            let candidate = text.get(index..index + TOTAL).unwrap_or("");
            let ok = candidate.len() == TOTAL
                && candidate.get(PREFIX.len()..).is_some_and(|tail| {
                    tail.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                });
            if ok && left_boundary_ok(text, index) && !is_placeholder(candidate) {
                push_finding(text, findings, SecretKind::GoogleApi, index, index + TOTAL);
                index += TOTAL;
                continue;
            }
        }
        index += 1;
    }
}

/// Detects JSON Web Tokens: three Base64URL segments starting with `eyJ`.
fn scan_jwts(text: &str, findings: &mut Vec<SecretFinding>) {
    const HEADER: &str = "eyJ";

    let mut cursor = 0usize;
    while let Some(rel) = text.get(cursor..).and_then(|rest| rest.find(HEADER)) {
        let start = cursor + rel;
        if !left_boundary_ok(text, start) {
            cursor = start + HEADER.len();
            continue;
        }
        let body = text.get(start..).unwrap_or("");
        let len = body
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'))
            .unwrap_or(body.len());
        let token = body.get(..len).unwrap_or("");
        let segments: Vec<&str> = token.split('.').collect();
        let valid = segments.len() == 3
            && segments.iter().all(|segment| {
                !segment.is_empty()
                    && segment
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            })
            && segments.first().is_some_and(|s| s.starts_with(HEADER))
            && token.len() >= 40;
        if valid {
            push_finding(text, findings, SecretKind::Jwt, start, start + len);
        }
        cursor = start + len.max(1);
    }
}

/// Detects generic `api_key = <high entropy value>` assignments.
fn scan_generic_assignments(text: &str, findings: &mut Vec<SecretFinding>) {
    const KEYWORDS: &[&str] = &[
        "api_key",
        "apikey",
        "api-key",
        "secret",
        "token",
        "password",
        "passwd",
        "access_key",
        "private_key",
        "client_secret",
    ];

    let mut line_start = 0usize;
    for line in text.split('\n') {
        let lower = line.to_ascii_lowercase();
        let keyword_hit = KEYWORDS.iter().any(|kw| lower.contains(kw));
        if !keyword_hit {
            line_start += line.len() + 1;
            continue;
        }

        let mut search_from = 0usize;
        while let Some(rel_sep) = line.get(search_from..).and_then(|s| s.find(['=', ':'])) {
            let sep = search_from + rel_sep;
            let before = line.get(..sep).unwrap_or("");
            let key_part = before
                .rsplit(['{', '[', ',', ';', '(', '\t'])
                .next()
                .unwrap_or(before);
            let key_lower = key_part.to_ascii_lowercase();
            let mut next_cursor = sep + 1;
            if KEYWORDS.iter().any(|kw| key_lower.contains(kw)) {
                let raw_value = line.get(sep + 1..).unwrap_or("");
                // Offset of the first non-whitespace byte of the raw value.
                let leading_ws = raw_value.len() - raw_value.trim_start().len();
                let after_ws = raw_value.get(leading_ws..).unwrap_or("");
                // Offset of the first byte after any opening quote.
                let quote_offset = after_ws
                    .find(|c: char| c != '"' && c != '\'' && c != '`')
                    .unwrap_or(after_ws.len());
                let unquoted = after_ws.get(quote_offset..).unwrap_or("");
                // Stop at the first structural delimiter so that JSON, YAML and
                // TOML punctuation is never swallowed by the redaction span.
                let value = match unquoted.find(['}', ']', ',', '"', '\'', '`', ')', ';']) {
                    Some(pos) => unquoted.get(..pos).unwrap_or(""),
                    None => unquoted,
                };
                let value = value.trim_end();
                let value_start = sep + 1 + leading_ws + quote_offset;
                if value.len() >= MIN_GENERIC_LEN
                    && !is_placeholder(value)
                    && !is_uuid(value)
                    && !is_file_path(value)
                    && !is_hex_digest(value)
                    && shannon_entropy(value) >= MIN_GENERIC_ENTROPY
                {
                    let start = line_start + value_start;
                    let end = start + value.len();
                    push_finding(text, findings, SecretKind::GenericHighEntropy, start, end);
                    next_cursor = value_start + value.len();
                }
            }
            search_from = next_cursor;
        }

        line_start += line.len() + 1;
    }
}

/// Returns `true` for obvious placeholder values.
fn is_placeholder(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "your_",
        "your-",
        "yourkey",
        "example",
        "changeme",
        "change_me",
        "placeholder",
        "dummy",
        "sample",
        "xxxx",
        "****",
        "redacted",
        "todo",
        "fixme",
        "<",
        ">",
        "${",
        "{{",
    ];
    if MARKERS.iter().any(|marker| lower.contains(marker)) {
        return true;
    }
    let stripped: String = value
        .chars()
        .filter(|c| !matches!(c, '*' | 'x' | 'X' | '.' | '-' | '_'))
        .collect();
    stripped.is_empty()
}

/// Returns `true` for a canonical UUID (`8-4-4-4-12` hex).
fn is_uuid(value: &str) -> bool {
    let groups: Vec<&str> = value.split('-').collect();
    let lengths = [8usize, 4, 4, 4, 12];
    groups.len() == 5
        && groups
            .iter()
            .zip(lengths.iter())
            .all(|(group, expected)| {
                group.len() == *expected && group.chars().all(|c| c.is_ascii_hexdigit())
            })
}

/// Returns `true` for values that look like filesystem paths.
fn is_file_path(value: &str) -> bool {
    value.contains('/') || value.contains('\\') || value.starts_with('~') || value.starts_with('.')
}

/// Returns `true` for an isolated hexadecimal digest (MD5/SHA-1/SHA-256).
fn is_hex_digest(value: &str) -> bool {
    matches!(value.len(), 32 | 40 | 64) && value.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{scan_secrets, SecretKind};

    /// Builds a synthetic token from a prefix and a repeated filler so that the
    /// test sources never contain a literal credential.
    fn token(prefix: &str, filler: &str, repeat: usize) -> String {
        let mut out = String::from(prefix);
        for _ in 0..repeat {
            out.push_str(filler);
        }
        out
    }

    #[test]
    fn scan_exact_signatures() {
        let openai = token("sk-", "aB3xY9zQ2wE4rT6yU8iO", 2);
        let anthropic = token("sk-ant-", "pL7mN4kR2sT9vW5xZ8cQ", 2);
        let github = token("ghp_", "aB3xY9zQ2wE4rT6yU8iO0pL7mN4k", 1);
        let github_pat = token("github_pat_", "aB3xY9zQ2wE4rT6yU8iO0pL7mN4k", 1);
        let aws = token("AKIA", "Q7RTZ2NPLK4MW8XV", 1);
        let slack = token("xoxb-", "123456789012-abcdefghijklmnop", 1);
        let google = token("AIza", "aB3xY9zQ2wE4rT6yU8iO0pL7mN4kR2sT9vW", 1);
        let jwt = concat!(
            "eyJhbGciOiJIUzI1NiJ9",
            ".eyJzdWIiOiIxMjM0NTY3ODkwIn0",
            ".dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U"
        );

        let text = format!(
            "openai = {openai}\nanthropic = {anthropic}\ngithub = {github}\n\
             github_pat = {github_pat}\naws = {aws}\nslack = {slack}\n\
             google = {google}\njwt = {jwt}\n"
        );

        let kinds: Vec<SecretKind> = scan_secrets(&text).iter().map(|f| f.kind).collect();
        assert!(kinds.contains(&SecretKind::OpenAi));
        assert!(kinds.contains(&SecretKind::GitHub));
        assert!(kinds.contains(&SecretKind::Aws));
        assert!(kinds.contains(&SecretKind::Slack));
        assert!(kinds.contains(&SecretKind::GoogleApi));
        assert!(kinds.contains(&SecretKind::Jwt));
    }

    #[test]
    fn scan_pem_and_url_credentials() {
        let dashes = "-".repeat(5);
        let begin = format!("{dashes}BEGIN RSA PRIVATE KEY{dashes}");
        let end = format!("{dashes}END RSA PRIVATE KEY{dashes}");
        let body = "MIIEowIBAAKCAQEA1234567890abcdefghijklmnopqrstuvwxyz";
        let pem = format!("{begin}\n{body}\n{end}\n");

        let findings = scan_secrets(&pem);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, SecretKind::PemPrivateKey);
        assert_eq!(findings[0].as_str(&pem), pem.trim_end());

        let password = "s3cr3tP4ssw0rd";
        let scheme = "postgres";
        let sep = "://";
        let user = "admin";
        let colon = ":";
        let at = "@";
        let host = "db.internal:5432/app";
        let url = format!("{scheme}{sep}{user}{colon}{password}{at}{host}");

        let findings = scan_secrets(&url);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, SecretKind::UrlCredentials);
        assert_eq!(findings[0].as_str(&url), password);
    }

    #[test]
    fn scan_generic_high_entropy() {
        let text = "API_KEY=a8Fk2LmQ9zXr4TbV7nPw\n";
        let findings = scan_secrets(text);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, SecretKind::GenericHighEntropy);
        assert_eq!(findings[0].as_str(text), "a8Fk2LmQ9zXr4TbV7nPw");

        let weak = "API_KEY=aaaaaaaaaaaaaaaaaaaaaaaa\n";
        assert!(scan_secrets(weak).is_empty());
    }

    #[test]
    fn scan_ignores_false_positives() {
        let text = concat!(
            "API_KEY=YOUR_API_KEY\n",
            "SECRET=example\n",
            "TOKEN=changeme\n",
            "PASSWORD=<token>\n",
            "API_KEY=***\n",
            "ID=550e8400-e29b-41d4-a716-446655440000\n",
            "PATH=/usr/local/bin/tool\n",
            "HASH=e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n",
        );
        assert!(scan_secrets(text).is_empty());
    }

    #[test]
    fn no_panic_unicode_and_large_input() {
        let unicode = "clé=🔐 très longue valeur ünïcode — 日本語のテキスト";
        let _ = scan_secrets(unicode);

        let mut large = String::with_capacity(200_000);
        for index in 0..5_000 {
            large.push_str("KEY_");
            large.push_str(&index.to_string());
            large.push_str("=value\n");
        }
        assert!(scan_secrets(&large).is_empty());

        let mut huge_secret = String::from("API_KEY=");
        huge_secret.push_str(&"aB3xY9zQ2wE4rT6yU8iO".repeat(10_000));
        let findings = scan_secrets(&huge_secret);
        assert_eq!(findings.len(), 1);
    }
}
