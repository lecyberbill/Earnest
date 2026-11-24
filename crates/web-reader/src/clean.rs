//! Noise removal and main-content selection.
//!
//! Two independent passes, both tolerant to malformed markup:
//!
//! 1. `strip_noise` drops HTML comments and the whole subtree of every
//!    decorative/interactive container (`script`, `style`, `noscript`, `svg`,
//!    `nav`, `footer`, `header`, `aside`).
//! 2. `select_main_content` narrows the document down to its `<main>` (or
//!    `<article>`) subtree when one exists.

use crate::metadata::{find_ci, next_tag};

/// Elements whose entire subtree is dropped from the extracted document.
const NOISE_TAGS: &[&str] = &[
    "script", "style", "noscript", "svg", "nav", "footer", "header", "aside",
];

/// Elements whose body is raw text: the first closing tag wins, because the
/// payload may contain `<` sequences that a tag scanner would misread.
const RAW_TEXT_TAGS: &[&str] = &["script", "style"];

/// Removes comments, noise subtrees and keeps only the main content.
pub fn clean_html(html: &str) -> String {
    let stripped = strip_noise(html);
    let focused = select_main_content(&stripped);
    // Second pass: guarantees that nothing nested in the selected subtree
    // survived, and that the result is idempotent.
    strip_noise(&focused).trim().to_string()
}

/// Removes every `<!-- ... -->` block (unterminated comments swallow the tail).
pub(crate) fn strip_comments(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;

    loop {
        match rest.find("<!--") {
            Some(index) => {
                out.push_str(&rest[..index]);
                let after = &rest[index + 4..];
                match after.find("-->") {
                    Some(close) => rest = &after[close + 3..],
                    None => return out,
                }
            }
            None => {
                out.push_str(rest);
                return out;
            }
        }
    }
}

/// Drops the noise subtrees listed in [`NOISE_TAGS`].
pub(crate) fn strip_noise(html: &str) -> String {
    let comments_free = strip_comments(html);
    let mut out = String::with_capacity(comments_free.len());
    let mut cursor = 0usize;
    let mut position = 0usize;

    while let Some(tag) = next_tag(&comments_free, position) {
        if tag.closing || !is_noise(tag.name) {
            position = tag.end;
            continue;
        }

        // Keep everything before the noise block, then jump past it.
        out.push_str(&comments_free[cursor..tag.start]);
        let block_end = if tag.self_closing {
            tag.end
        } else if RAW_TEXT_TAGS
            .iter()
            .any(|name| tag.name.eq_ignore_ascii_case(name))
        {
            raw_text_end(&comments_free, tag.name, tag.end)
        } else {
            matching_close_end(&comments_free, tag.name, tag.end)
        };

        cursor = block_end;
        position = block_end;
    }

    out.push_str(&comments_free[cursor..]);
    out
}

/// Returns the inner HTML of `<main>` (or `<article>`), or the input unchanged.
pub(crate) fn select_main_content(html: &str) -> String {
    for name in ["main", "article"] {
        if let Some((inner_start, inner_end)) = element_inner(html, name) {
            let inner = &html[inner_start..inner_end.max(inner_start)];
            if !inner.trim().is_empty() {
                return inner.to_string();
            }
        }
    }

    html.to_string()
}

fn is_noise(name: &str) -> bool {
    NOISE_TAGS
        .iter()
        .any(|noise| name.eq_ignore_ascii_case(noise))
}

/// Byte index of the first known tag named `name`, with its inner bounds.
fn element_inner(html: &str, name: &str) -> Option<(usize, usize)> {
    let mut position = 0usize;

    while let Some(tag) = next_tag(html, position) {
        if !tag.closing && !tag.self_closing && tag.name.eq_ignore_ascii_case(name) {
            let inner_start = tag.end;
            let inner_end = matching_close_start(html, name, inner_start);
            return Some((inner_start, inner_end));
        }
        position = tag.end;
    }

    None
}

/// Byte index of the closing tag matching an element opened before `from`.
fn matching_close_start(html: &str, name: &str, from: usize) -> usize {
    let mut depth = 1usize;
    let mut position = from;

    while let Some(tag) = next_tag(html, position) {
        if tag.name.eq_ignore_ascii_case(name) {
            if tag.closing {
                depth -= 1;
                if depth == 0 {
                    return tag.start;
                }
            } else if !tag.self_closing {
                depth += 1;
            }
        }
        position = tag.end;
    }

    html.len()
}

/// Byte index just past the closing tag matching an opened element.
fn matching_close_end(html: &str, name: &str, from: usize) -> usize {
    let close_start = matching_close_start(html, name, from);
    if close_start >= html.len() {
        return html.len();
    }

    match html[close_start..].find('>') {
        Some(offset) => close_start + offset + 1,
        None => html.len(),
    }
}

/// For raw-text elements, the payload ends at the first `</name`.
fn raw_text_end(html: &str, name: &str, from: usize) -> usize {
    let needle = format!("</{}", name);
    match find_ci(&html[from..], &needle) {
        Some(offset) => {
            let close_start = from + offset;
            match html[close_start..].find('>') {
                Some(gt) => close_start + gt + 1,
                None => html.len(),
            }
        }
        None => html.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_blocks_and_html_comments_are_fully_stripped() {
        let html = "<html><head><title>T</title><style>body { color: red }</style></head>\
<body><!-- a comment with <p>markup</p> --><nav><a href='/'>Home</a></nav>\
<header>Site header</header><main><h1>Keep</h1><p>Visible text</p>\
<script>var hidden = 1; if (a < b) {}</script><noscript>Enable JS</noscript>\
<svg><circle r='1'/></svg><aside>Sidebar</aside><footer>Copyright</footer></main></body></html>";

        let cleaned = clean_html(html);

        for leaked in [
            "color: red",
            "a comment",
            ">Home<",
            "Site header",
            "hidden",
            "Enable JS",
            "circle",
            "Sidebar",
            "Copyright",
            "<!--",
        ] {
            assert!(!cleaned.contains(leaked), "noise leaked: {}", leaked);
        }

        assert!(cleaned.contains("Keep"));
        assert!(cleaned.contains("Visible text"));

        // Idempotent and safe on adversarially broken markup.
        assert_eq!(clean_html(&cleaned), cleaned);
        assert_eq!(strip_comments("a<!-- x -->b"), "ab");
        assert_eq!(strip_comments("a<!-- unterminated"), "a");
        assert!(clean_html("<script>").is_empty());
    }

    #[test]
    fn main_or_article_subtree_prioritized_over_wrapping_document() {
        let html = "<body><p>Wrapper intro</p><main><h1>Main title</h1><p>Main body</p></main>\
<p>Wrapper outro</p></body>";
        let cleaned = clean_html(html);
        assert!(cleaned.contains("Main title"));
        assert!(cleaned.contains("Main body"));
        assert!(!cleaned.contains("Wrapper intro"));
        assert!(!cleaned.contains("Wrapper outro"));

        let html = "<div><p>Before</p><article><p>Article body</p></article><p>After</p></div>";
        let cleaned = clean_html(html);
        assert!(cleaned.contains("Article body"));
        assert!(!cleaned.contains("Before"));
        assert!(!cleaned.contains("After"));

        // `main` wins over `article`, and nested markup keeps its depth.
        let html = "<main><article><p>Nested</p></article></main>";
        let cleaned = clean_html(html);
        assert!(cleaned.contains("Nested"));

        // No main/article at all: the whole document is kept.
        let html = "<div><p>Only body</p></div>";
        assert!(clean_html(html).contains("Only body"));

        // Empty main falls back to the document.
        let html = "<main>   </main><p>Fallback</p>";
        assert!(clean_html(html).contains("Fallback"));
    }
}
