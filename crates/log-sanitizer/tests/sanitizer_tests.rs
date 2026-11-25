use log_sanitizer::{SecretKind, contains_secrets, sanitize_report, sanitize_text};

#[test]
fn redacts_api_keys_and_bearer_tokens() {
    let raw = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.abc\nOPENAI=sk-proj-1234567890abcdef\nGH=ghp_1234567890abcdef1234\n";
    let report = sanitize_report(raw);
    assert!(!report.sanitized.contains("eyJhbGciOi"));
    assert!(!report.sanitized.contains("sk-proj-1234567890abcdef"));
    assert!(!report.sanitized.contains("ghp_1234567890abcdef1234"));
    assert!(report.sanitized.contains("[REDACTED:BEARER_TOKEN]"));
    assert!(report.sanitized.contains("[REDACTED:API_KEY]"));
    assert!(report.total_redacted() >= 3);
}

#[test]
fn redacts_pem_private_key_blocks() {
    let raw = "log start\n-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA123456\n-----END RSA PRIVATE KEY-----\nlog end\n";
    let report = sanitize_report(raw);
    assert!(!report.sanitized.contains("MIIEowIBAAKCAQEA"));
    assert!(report.sanitized.contains("[REDACTED:PRIVATE_KEY]"));
    assert!(report.sanitized.contains("log start"));
    assert!(report.sanitized.contains("log end"));
    assert_eq!(report.redactions[0].kind, SecretKind::PrivateKeyBlock);
}

#[test]
fn redacts_env_assignments_and_url_credentials() {
    let raw = "DATABASE_URL=postgres://admin:supersecret99@db.internal:5432/app\nSTRIPE_SECRET_KEY=\"whsec_1234567890\"\n";
    let cleaned = sanitize_text(raw);
    assert!(!cleaned.contains("supersecret99"));
    assert!(!cleaned.contains("whsec_1234567890"));
    assert!(cleaned.contains("postgres://admin:[REDACTED:URL_CREDENTIAL]@db.internal:5432/app"));
    assert!(cleaned.contains("[REDACTED:SECRET]"));
}

#[test]
fn avoids_false_positives_on_normal_code_and_placeholders() {
    let safe = "fn main() {\n    let url = \"https://api.example.com/v1/models\";\n    let api_key = std::env::var(\"OPENAI_API_KEY\");\n}\nOPENAI_API_KEY=$MY_VAR\n";
    assert!(!contains_secrets(safe));
    assert_eq!(sanitize_text(safe), safe);
}

#[test]
fn redacts_jwt_and_inline_high_entropy_secrets_via_dotenv_shield() {
    let raw = "{\"jwt\":\"eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c\",\"client_secret\":\"a8Fk2LmQ9zXr4TbV7nPw9QxZ\"}";
    let report = sanitize_report(raw);
    assert!(!report.sanitized.contains("SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"));
    assert!(!report.sanitized.contains("a8Fk2LmQ9zXr4TbV7nPw9QxZ"));
    assert!(report.total_redacted() >= 2);
}
