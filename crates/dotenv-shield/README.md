# dotenv_shield

A strict `.env` parser combined with a high-precision secret detector and
redactor, written in pure `std` Rust.

* **Zero dependencies** — `[dependencies]` is empty.
* **`#![forbid(unsafe_code)]`** and **`#![deny(missing_docs)]`**.
* No panics on Unicode, malformed or very large inputs.
* Clean under `cargo clippy --all-targets -- -D warnings`.

## Installation

```toml
[dependencies]
dotenv_shield = "0.1"
```

## Parsing `.env` files

```rust
use dotenv_shield::{parse_dotenv, resolve_entries};

let input = r#"
# database
export DB_HOST=localhost
DB_PORT=5432
DB_NAME='my db'
DB_PASS="s3cr\t"
"#;

let mut entries = parse_dotenv(input).expect("valid dotenv");
resolve_entries(&mut entries);

assert_eq!(entries[0].key, "DB_HOST");
assert_eq!(entries[0].value, "localhost");
assert_eq!(entries[2].value, "my db");
```

Supported syntax:

| Form | Behaviour |
| --- | --- |
| `KEY=VALUE` | Unquoted value, inline `#` comment stripped |
| `export KEY=VALUE` | Same, with the `export` prefix ignored |
| `KEY='...'` | Single quotes, taken literally |
| `KEY="..."` | Double quotes, `\n` `\r` `\t` `\"` `\\` escapes, multi-line |
| `${VAR}` / `$VAR` | Interpolated by `resolve_entries` from previous keys |

Errors are reported as `DotenvError` values carrying the **1-based line
number** of the offending line:

```rust
use dotenv_shield::parse_dotenv;

let err = parse_dotenv("GOOD=1\n1BAD=2\n").unwrap_err();
assert_eq!(err.line(), 2);
```

Rejected inputs: invalid keys, unterminated quotes, NUL bytes, dangling
escapes and lines that are not assignments.

## Detecting secrets

```rust
use dotenv_shield::{scan_secrets, SecretKind};

let text = "OPENAI_API_KEY=sk-...";
for finding in scan_secrets(text) {
    println!("{} at line {}", finding.kind.as_str(), finding.line);
}
```

Detected signatures:

* OpenAI / Anthropic / DeepSeek `sk-...` keys
* GitHub `ghp_...` and `github_pat_...` tokens
* AWS access key ids (`AKIA` + 16 uppercase alphanumerics)
* Slack `xox[baprs]-...` tokens
* Google API keys (`AIza` + 35 URL-safe characters)
* JSON Web Tokens (three Base64URL segments starting with `eyJ`)
* PEM private key blocks
* URLs carrying credentials (`scheme://user:password@host`)
* Generic high-entropy assignments (`api_key`, `secret`, `token`,
  `password`, ...) with a Shannon entropy of at least **3.8 bits/char** over at
  least **20 characters**

Obvious placeholders (`YOUR_API_KEY`, `example`, `changeme`, `<token>`, `***`),
bare UUIDs, filesystem paths and isolated SHA-256 hex digests are ignored.

## Redacting secrets

```rust
use dotenv_shield::redact_secrets;

let json = r#"{"api_key": "a8Fk2LmQ9zXr4TbV7nPw"}"#;
assert_eq!(
    redact_secrets(json),
    r#"{"api_key": "[REDACTED:generic_high_entropy]"}"#
);
```

`redact_secrets` only rewrites the secret spans, so JSON, YAML, TOML and URL
structure are preserved byte for byte:

```text
postgres://admin:[REDACTED:url_credentials]@db.internal:5432/app
```

## Generating a `.env.example`

```rust
use dotenv_shield::mask_dotenv_template;

let input = "# service configuration\nPORT=8080\nRUST_LOG=info\nAPI_KEY=sk-...\n";
let template = mask_dotenv_template(input).unwrap();
assert!(template.contains("PORT=8080"));
assert!(template.contains("API_KEY=<YOUR_API_KEY>"));
```

Comments, blank lines and non-sensitive keys are preserved; sensitive values
become `<YOUR_KEY_NAME>`.

## License

Licensed under either of MIT or Apache-2.0 at your option.
