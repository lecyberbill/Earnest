//! UTF-8 safe truncation of the generated Markdown.
//!
//! The cut is always taken on a character boundary *and* preferably on a line
//! boundary. If the cut lands inside a fenced code block, the fence is closed
//! so the truncated document stays valid Markdown.

/// Text appended after a truncated document.
pub const TRUNCATION_MARKER: &str = "[... truncated ...]";

/// Truncates `markdown` to at most `max_chars` characters.
///
/// Returns the input unchanged (byte-for-byte) when it already fits.
pub fn truncate_markdown(markdown: &str, max_chars: usize) -> String {
    if markdown.chars().count() <= max_chars {
        return markdown.to_string();
    }

    // Largest byte index reachable with at most `max_chars` characters.
    let mut cut = 0usize;
    for (taken, (index, ch)) in markdown.char_indices().enumerate() {
        if taken >= max_chars {
            break;
        }
        cut = index + ch.len_utf8();
    }

    // Prefer cutting on the last complete line.
    let head = &markdown[..cut];
    let head = match head.rfind('\n') {
        Some(position) if position > 0 => &head[..position],
        _ => head,
    };

    let mut out = head.trim_end().to_string();
    if out.is_empty() {
        return TRUNCATION_MARKER.to_string();
    }

    if has_open_fence(&out) {
        out.push_str("\n```");
    }

    out.push_str("\n\n");
    out.push_str(TRUNCATION_MARKER);
    out
}

/// `true` when the document ends inside a fenced code block.
fn has_open_fence(markdown: &str) -> bool {
    let mut open = false;

    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            open = !open;
        }
    }

    open
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_safe_truncation_boundary_fence_closure_and_marker() {
        // Fits: returned untouched.
        assert_eq!(truncate_markdown("abc", 10), "abc");
        assert_eq!(truncate_markdown("", 0), "");

        // Multi-byte characters must never be split.
        let markdown = "# Titre\n\nhéllo wörld — emoji 😀 中文 tail";
        let truncated = truncate_markdown(markdown, 12);
        assert!(truncated.ends_with(TRUNCATION_MARKER));
        assert!(truncated.starts_with("# Titre"));
        assert!(truncated.is_char_boundary(truncated.len()));
        assert!(truncated.chars().count() < markdown.chars().count() + 32);

        // A cut inside an open fence is repaired.
        let markdown = "```rust\nfn main() {}\n```\n\ntail";
        let truncated = truncate_markdown(markdown, 12);
        assert!(truncated.starts_with("```rust"));
        assert!(truncated.ends_with(TRUNCATION_MARKER));
        let fences = truncated
            .lines()
            .filter(|line| line.trim_start().starts_with("```"))
            .count();
        assert_eq!(fences % 2, 0, "unbalanced fences: {:?}", truncated);
        assert_eq!(truncated, format!("```rust\n```\n\n{}", TRUNCATION_MARKER));

        // Zero budget degrades to the marker alone.
        assert_eq!(truncate_markdown("abc", 0), TRUNCATION_MARKER);

        // Non-boundary budgets are honoured without panicking.
        let markdown = "é".repeat(10);
        let truncated = truncate_markdown(&markdown, 5);
        assert!(truncated.ends_with(TRUNCATION_MARKER));
    }
}
