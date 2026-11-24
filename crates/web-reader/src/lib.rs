//! `web_reader` — turn raw HTML into clean, structured Markdown for LLM agents.
//!
//! The crate is intentionally dependency-free (std only): no network access, no
//! external DOM parser. The pipeline is linear and allocates one string per
//! stage:
//!
//! ```text
//! html ──► metadata ──► head removal ──► noise/main selection ──► markdown ──► truncation
//! ```
//!
//! Every stage is tolerant to malformed markup: unexpected input degrades into
//! slightly worse output, never into a panic.
//!
//! ```
//! use web_reader::extract_markdown;
//!
//! let page = extract_markdown(
//!     "<html><head><title>Demo</title></head><body><main><h1>Hi</h1><p>Body</p></main></body></html>",
//!     10_000,
//! );
//! assert_eq!(page.metadata.title.as_deref(), Some("Demo"));
//! assert!(page.markdown.contains("# Hi"));
//! ```

pub mod clean;
pub mod convert;
pub mod entities;
pub mod metadata;
pub mod truncate;

use crate::metadata::{extract_metadata, next_tag};

/// Metadata collected from the document head.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageMetadata {
    /// Text of the first `<title>` element, whitespace-normalized.
    pub title: Option<String>,
    /// `<meta name="description">`, falling back to `og:description`.
    pub description: Option<String>,
}

/// Result of [`extract_markdown`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtractedPage {
    /// Title and description found in the document.
    pub metadata: PageMetadata,
    /// Converted Markdown, ending with a single newline when non-empty.
    pub markdown: String,
    /// `true` when `markdown` had to be shortened to respect the budget.
    pub truncated: bool,
}

/// Converts a raw HTML document into Markdown for an LLM prompt.
///
/// The output is guaranteed to be at most `max_chars` **characters** long
/// (never bytes, so multi-byte characters are never split). When the document
/// fits, it is returned untouched and [`ExtractedPage::truncated`] is `false`.
pub fn extract_markdown(html: &str, max_chars: usize) -> ExtractedPage {
    let metadata = extract_metadata(html);

    // `<head>` only carries metadata: its text must never reach the Markdown.
    let body = strip_head(html);
    let cleaned = clean::clean_html(&body);
    let markdown = convert::to_markdown(&cleaned);

    let truncated = markdown.chars().count() > max_chars;
    let markdown = truncate::truncate_markdown(&markdown, max_chars);

    ExtractedPage {
        metadata,
        markdown,
        truncated,
    }
}

/// Removes the whole `<head>…</head>` subtree (unterminated heads eat the tail).
fn strip_head(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut cursor = 0usize;
    let mut position = 0usize;

    while let Some(tag) = next_tag(html, position) {
        if !tag.closing && tag.name.eq_ignore_ascii_case("head") {
            out.push_str(&html[cursor..tag.start]);
            let end = head_end(html, tag.end);
            cursor = end;
            position = end;
            continue;
        }
        position = tag.end;
    }

    out.push_str(&html[cursor..]);
    out
}

/// Byte index just past the `</head>` matching an already opened `<head>`.
fn head_end(html: &str, from: usize) -> usize {
    let mut depth = 1usize;
    let mut position = from;

    while let Some(tag) = next_tag(html, position) {
        if tag.name.eq_ignore_ascii_case("head") {
            if tag.closing {
                depth -= 1;
                if depth == 0 {
                    return tag.end;
                }
            } else if !tag.self_closing {
                depth += 1;
            }
        }
        position = tag.end;
    }

    html.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::truncate::TRUNCATION_MARKER;

    #[test]
    fn public_api_signature_and_zero_dependencies() {
        // The public entry point keeps its documented signature.
        let entry: fn(&str, usize) -> ExtractedPage = extract_markdown;
        let page = entry(
            "<html><head><title>Doc title</title>\
             <meta name=\"description\" content=\"Doc description\"></head>\
             <body><p>Body text</p></body></html>",
            10_000,
        );

        assert_eq!(page.metadata.title.as_deref(), Some("Doc title"));
        assert_eq!(page.metadata.description.as_deref(), Some("Doc description"));
        assert_eq!(page.markdown, "Body text\n");
        assert!(!page.truncated);
        assert!(!page.markdown.contains("Doc title"));

        // `PageMetadata` and `ExtractedPage` are plain, inspectable structs.
        let metadata = PageMetadata {
            title: Some("t".to_string()),
            description: None,
        };
        let extracted = ExtractedPage {
            metadata: metadata.clone(),
            markdown: "x".to_string(),
            truncated: false,
        };
        assert_eq!(extracted.metadata, metadata);
        assert_eq!(ExtractedPage::default(), ExtractedPage::default());
        assert_eq!(PageMetadata::default().title, None);

        // Zero external dependency: the manifest declares no dependency table.
        let manifest = include_str!("../Cargo.toml");
        let stripped: String = manifest
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in [
            "[dependencies]",
            "[dev-dependencies]",
            "[build-dependencies]",
            "dependencies.",
        ] {
            assert!(
                !stripped.contains(forbidden),
                "unexpected dependency table: {}",
                forbidden
            );
        }
        assert!(stripped.contains("edition = \"2021\""));
    }

    #[test]
    fn malformed_and_adversarial_inputs_never_panic() {
        let adversarial: Vec<String> = vec![
            String::new(),
            "<".to_string(),
            "<>".to_string(),
            "<<<<>>>>".to_string(),
            "<!--".to_string(),
            "<!-- never closed".to_string(),
            "<script>".to_string(),
            "<style>a{".to_string(),
            "<pre>".to_string(),
            "<pre><code>".to_string(),
            "<table><tr><td>".to_string(),
            "<ul><li>a<li>b".to_string(),
            "<a href=".to_string(),
            "<a href=\"javascript:alert(1)".to_string(),
            "<h1>unclosed".to_string(),
            "</html></body></head></div>".to_string(),
            "<title>".to_string(),
            "<head>".to_string(),
            "<head><meta name=description content=x>".to_string(),
            "<p>&#0;&#x110000;&#99999999999999999;&bogus;</p>".to_string(),
            "<p>emoji 😀 中文 accents éàü</p>".to_string(),
            format!("<div>{}</div>", "<div>".repeat(200)),
            format!("<p>{}</p>", "text ".repeat(500)),
            "<img src=javascript:alert(1) alt='x'>".to_string(),
            "<blockquote><blockquote>".to_string(),
            "<svg><text>never mind</svg>".to_string(),
        ];

        for html in adversarial {
            let page = extract_markdown(&html, 200);
            // Purely structural invariants: no panic, bounded length, no markers.
            assert!(page.markdown.chars().count() <= 200 + TRUNCATION_MARKER.chars().count() + 8);
            assert!(!page.markdown.contains("\u{FFFD}\u{FFFD}\u{FFFD}"));
        }
    }

    #[test]
    fn determinism_empty_input_and_no_truncation_when_fits() {
        let html = "<main><h2>Title</h2><p>Some <strong>bold</strong> text.</p></main>";

        let first = extract_markdown(html, 10_000);
        let second = extract_markdown(html, 10_000);
        assert_eq!(first, second);
        assert_eq!(first.markdown, "## Title\n\nSome **bold** text.\n");
        assert!(!first.truncated);
        assert!(!first.markdown.contains(TRUNCATION_MARKER));

        // Exactly at the budget is not a truncation.
        let limit = first.markdown.chars().count();
        let exact = extract_markdown(html, limit);
        assert_eq!(exact.markdown, first.markdown);
        assert!(!exact.truncated);

        // Empty input yields an empty document, never a panic.
        let empty = extract_markdown("", 500);
        assert_eq!(empty, ExtractedPage::default());
        assert_eq!(empty.metadata, PageMetadata::default());
    }
}
