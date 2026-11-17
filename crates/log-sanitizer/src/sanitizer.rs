//! Secret detection and redaction engine (`std`-only).

/// Category of detected secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretKind {
    PrivateKeyBlock,
    BearerToken,
    ApiKeyToken,
    EnvAssignment,
    UrlCredential,
}

impl SecretKind {
    fn tag(self) -> &'static str {
        match self {
            Self::PrivateKeyBlock => "[REDACTED:PRIVATE_KEY]",
            Self::BearerToken => "[REDACTED:BEARER_TOKEN]",
            Self::ApiKeyToken => "[REDACTED:API_KEY]",
            Self::EnvAssignment => "[REDACTED:SECRET]",
            Self::UrlCredential => "[REDACTED:URL_CREDENTIAL]",
        }
    }
}

/// Metadata for a single applied redaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redaction {
    pub kind: SecretKind,
    pub count: usize,
}

/// Result of sanitizing a text payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizeReport {
    pub sanitized: String,
    pub redactions: Vec<Redaction>,
}

impl SanitizeReport {
    /// Total number of redacted secret occurrences.
    pub fn total_redacted(&self) -> usize {
        self.redactions.iter().map(|r| r.count).sum()
    }
}

/// Sanitizes `input` and returns only the redacted string.
pub fn sanitize_text(input: &str) -> String {
    sanitize_report(input).sanitized
}

/// Returns `true` if `input` contains at least one detectable secret.
pub fn contains_secrets(input: &str) -> bool {
    sanitize_report(input).total_redacted() > 0
}

/// Sanitizes `input` and returns both the cleaned text and a summary of redactions.
pub fn sanitize_report(input: &str) -> SanitizeReport {
    let mut redactions = Vec::new();

    let (step1, pem_count) = redact_pem_blocks(input);
    if pem_count > 0 {
        redactions.push(Redaction { kind: SecretKind::PrivateKeyBlock, count: pem_count });
    }

    let (step2, bearer_count) = redact_bearer_tokens(&step1);
    if bearer_count > 0 {
        redactions.push(Redaction { kind: SecretKind::BearerToken, count: bearer_count });
    }

    let (step3, url_count) = redact_url_credentials(&step2);
    if url_count > 0 {
        redactions.push(Redaction { kind: SecretKind::UrlCredential, count: url_count });
    }

    let (step4, prefix_count) = redact_known_prefixes(&step3);
    if prefix_count > 0 {
        redactions.push(Redaction { kind: SecretKind::ApiKeyToken, count: prefix_count });
    }

    let (step5, env_count) = redact_env_assignments(&step4);
    if env_count > 0 {
        redactions.push(Redaction { kind: SecretKind::EnvAssignment, count: env_count });
    }

    SanitizeReport {
        sanitized: step5,
        redactions,
    }
}

fn redact_pem_blocks(input: &str) -> (String, usize) {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    let mut count = 0;

    while let Some(start) = rest.find("-----BEGIN ") {
        let after_begin = &rest[start..];
        let Some(header_end) = after_begin.find("-----") else {
            break;
        };
        let header_line_end = after_begin[header_end + 5..].find("-----").map(|i| header_end + 5 + i + 5);
        let Some(end_marker_rel) = after_begin.find("-----END ") else {
            break;
        };
        let after_end = &after_begin[end_marker_rel..];
        let Some(tail_dashes) = after_end[9..].find("-----") else {
            break;
        };
        let _ = header_line_end;
        let block_end = start + end_marker_rel + 9 + tail_dashes + 5;
        out.push_str(&rest[..start]);
        out.push_str(SecretKind::PrivateKeyBlock.tag());
        rest = &rest[block_end..];
        count += 1;
    }

    out.push_str(rest);
    (out, count)
}

fn redact_bearer_tokens(input: &str) -> (String, usize) {
    let mut out = String::with_capacity(input.len());
    let mut count = 0;
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let rest = &input[i..];
        let matched = ["Bearer ", "bearer ", "Basic ", "basic "]
            .iter()
            .find(|p| rest.starts_with(**p));

        if let Some(prefix) = matched {
            let after = &rest[prefix.len()..];
            let tok_len = take_token_len(after);
            if tok_len >= 8 && !after.starts_with("[REDACTED:") {
                out.push_str(prefix);
                out.push_str(SecretKind::BearerToken.tag());
                i += prefix.len() + tok_len;
                count += 1;
                continue;
            }
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }

    (out, count)
}

fn redact_url_credentials(input: &str) -> (String, usize) {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    let mut count = 0;

    while let Some(scheme_pos) = rest.find("://") {
        let after_scheme = &rest[scheme_pos + 3..];
        let auth_end = after_scheme.find(['@', '/', ' ', '\n', '\r', '\t', '"', '\'']);
        if let Some(at_rel) = auth_end {
            if after_scheme.as_bytes()[at_rel] == b'@' {
                let creds = &after_scheme[..at_rel];
                if let Some(colon) = creds.find(':') {
                    let user = &creds[..colon];
                    let pass = &creds[colon + 1..];
                    if !user.is_empty() && !pass.is_empty() && !pass.starts_with("[REDACTED:") {
                        out.push_str(&rest[..scheme_pos + 3]);
                        out.push_str(user);
                        out.push(':');
                        out.push_str(SecretKind::UrlCredential.tag());
                        out.push('@');
                        rest = &after_scheme[at_rel + 1..];
                        count += 1;
                        continue;
                    }
                }
            }
        }
        out.push_str(&rest[..scheme_pos + 3]);
        rest = after_scheme;
    }

    out.push_str(rest);
    (out, count)
}

const KNOWN_KEY_PREFIXES: &[&str] = &[
    "sk-proj-",
    "sk-ant-",
    "sk-",
    "github_pat_",
    "ghp_",
    "gho_",
    "ghu_",
    "AIza",
    "AKIA",
    "xoxb-",
    "xoxp-",
    "glpat-",
];

fn redact_known_prefixes(input: &str) -> (String, usize) {
    let mut out = String::with_capacity(input.len());
    let mut count = 0;
    let mut i = 0;

    while i < input.len() {
        let rest = &input[i..];
        let prev_ok = i == 0 || !is_ident_char(input[..i].chars().next_back().unwrap());

        if prev_ok {
            if let Some(prefix) = KNOWN_KEY_PREFIXES.iter().find(|p| rest.starts_with(**p)) {
                let tok_len = take_token_len(rest);
                if tok_len >= prefix.len() + 8 {
                    out.push_str(SecretKind::ApiKeyToken.tag());
                    i += tok_len;
                    count += 1;
                    continue;
                }
            }
        }

        let ch = rest.chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }

    (out, count)
}

const SENSITIVE_ENV_KEYS: &[&str] = &[
    "API_KEY",
    "SECRET_KEY",
    "ACCESS_KEY",
    "PRIVATE_KEY",
    "AUTH_TOKEN",
    "ACCESS_TOKEN",
    "REFRESH_TOKEN",
    "CLIENT_SECRET",
    "PASSWORD",
    "PASSWD",
    "TOKEN",
    "SECRET",
];

fn redact_env_assignments(input: &str) -> (String, usize) {
    let mut out = String::with_capacity(input.len());
    let mut count = 0;

    for (idx, line) in input.split_inclusive('\n').enumerate() {
        let _ = idx;
        if let Some((redacted_line, changed)) = try_redact_env_line(line) {
            out.push_str(&redacted_line);
            if changed {
                count += 1;
            }
        } else {
            out.push_str(line);
        }
    }

    (out, count)
}

fn try_redact_env_line(line: &str) -> Option<(String, bool)> {
    let has_nl = line.ends_with('\n');
    let body = line.strip_suffix('\n').unwrap_or(line);
    let has_cr = body.ends_with('\r');
    let body = body.strip_suffix('\r').unwrap_or(body);

    let sep_pos = body.find('=').or_else(|| body.find(':'))?;
    let sep = body.as_bytes()[sep_pos] as char;
    let lhs = body[..sep_pos].trim();
    let rhs = body[sep_pos + 1..].trim();

    let raw_key = lhs
        .trim_start_matches("export ")
        .trim()
        .trim_matches(|c| c == '"' || c == '\'');

    // Must be a single env/config key identifier (avoids false positives on `let api_key = ...`).
    if raw_key.is_empty()
        || !raw_key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return None;
    }

    let key_upper = raw_key.to_ascii_uppercase();
    let is_sensitive = SENSITIVE_ENV_KEYS.iter().any(|k| key_upper.ends_with(k) || key_upper.contains(k));
    if !is_sensitive {
        return None;
    }

    let unquoted = rhs.trim_matches(|c| c == '"' || c == '\'' || c == ',');
    if unquoted.is_empty()
        || unquoted.starts_with("[REDACTED:")
        || unquoted == "null"
        || unquoted == "None"
        || unquoted.starts_with('$')
        || unquoted.contains("env::var")
        || unquoted.contains("getenv")
        || unquoted.contains("process.env")
    {
        return None;
    }

    let mut rebuilt = format!("{}{} {}", &body[..sep_pos], sep, SecretKind::EnvAssignment.tag());
    if body.trim_end().ends_with(',') {
        rebuilt.push(',');
    }
    if has_cr {
        rebuilt.push('\r');
    }
    if has_nl {
        rebuilt.push('\n');
    }
    Some((rebuilt, true))
}

fn take_token_len(s: &str) -> usize {
    s.char_indices()
        .take_while(|(_, c)| is_token_char(*c))
        .map(|(idx, c)| idx + c.len_utf8())
        .last()
        .unwrap_or(0)
}

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+' | '/' | '=')
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}
