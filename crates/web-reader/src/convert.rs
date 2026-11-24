//! HTML -> Markdown conversion.
//!
//! The converter walks the cleaned HTML once. Block-level elements (headings,
//! paragraphs, lists, code blocks, tables, quotes) drive the layout while
//! inline elements (emphasis, code, links, images) are rendered recursively.
//! No DOM is ever built: only the byte positions reported by `next_tag` are
//! used, so malformed markup degrades instead of panicking.

use crate::entities::{collapse_whitespace, decode};
use crate::metadata::{attr, next_tag, Tag};

/// Block-level containers that always start a new block in the output.
const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "center",
    "dd",
    "details",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "html",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "ul",
];

/// Converts a cleaned HTML fragment into normalized Markdown.
pub fn to_markdown(html: &str) -> String {
    let mut converter = Converter {
        src: html,
        out: String::new(),
        lists: Vec::new(),
        tight: 0,
    };
    converter.render_block_range(0, html.len());
    normalize(converter.out)
}

/// One list nesting level (only its depth matters for indentation).
struct Converter<'a> {
    src: &'a str,
    out: String,
    /// Stack of enclosing lists; its length gives the indentation depth.
    lists: Vec<bool>,
    /// Greater than zero while rendering the *contents* of a list item,
    /// where block boundaries become line breaks instead of blank lines.
    tight: usize,
}

impl<'a> Converter<'a> {
    // ---------------------------------------------------------------- blocks

    fn render_block_range(&mut self, start: usize, end: usize) {
        let src = self.src;
        let mut pos = start;

        while pos < end {
            let tag = match next_tag(src, pos) {
                Some(tag) if tag.start < end && tag.end <= end => tag,
                _ => {
                    self.push_block_text(pos, end);
                    return;
                }
            };

            self.push_block_text(pos, tag.start);
            pos = tag.end;

            if tag.closing {
                if is_block(tag.name) {
                    self.end_block();
                }
                continue;
            }

            let name = tag.name.to_ascii_lowercase();

            match name.as_str() {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    let level = (name.as_bytes()[1] - b'0') as usize;
                    let (inner_end, after) = bounds(src, &name, tag.end, end);
                    let text = collapse_whitespace(&self.inline(tag.end, inner_end));
                    if !text.is_empty() {
                        self.start_block();
                        self.out.push_str(&"#".repeat(level));
                        self.out.push(' ');
                        self.out.push_str(&text);
                        self.end_block();
                    }
                    pos = after;
                }
                "p" => self.start_block(),
                "br" => {
                    self.out.push('\n');
                    let indent = "  ".repeat(self.lists.len());
                    self.out.push_str(&indent);
                }
                "hr" => {
                    self.start_block();
                    self.out.push_str("---");
                    self.end_block();
                }
                "ul" | "ol" => {
                    let (inner_end, after) = bounds(src, &name, tag.end, end);
                    self.render_list(tag.end, inner_end, name == "ol");
                    pos = after;
                }
                "pre" => {
                    let (inner_end, after) = bounds(src, "pre", tag.end, end);
                    let tag_ref = &tag;
                    self.render_pre(tag_ref, tag.end, inner_end);
                    pos = after;
                }
                "table" => {
                    let (inner_end, after) = bounds(src, "table", tag.end, end);
                    self.render_table(tag.end, inner_end);
                    pos = after;
                }
                "blockquote" => {
                    let (inner_end, after) = bounds(src, "blockquote", tag.end, end);
                    self.render_quote(tag.end, inner_end);
                    pos = after;
                }
                // Defensive net: raw subtrees never reach the text pipeline.
                "script" | "style" | "noscript" | "svg" | "template" => {
                    let close = matching_close_end(src, &name, tag.end);
                    pos = close.min(end);
                }
                "img" => {
                    let rendered = self.inline(tag.start, tag.end);
                    self.out.push_str(&rendered);
                }
                other if is_block(other) => {}
                _ => {
                    // Inline element met at block level: render it in place.
                    if let Some(after) = element_end(src, &name, tag.end) {
                        if after <= end {
                            let rendered = self.inline(tag.start, after);
                            if rendered.is_empty() {
                                // An inline element holding only whitespace still
                                // separates the two surrounding text runs.
                                if src[tag.start..after].chars().any(|ch| ch.is_whitespace())
                                    && !self.out.is_empty()
                                    && !self.out.ends_with(' ')
                                    && !self.out.ends_with('\n')
                                {
                                    self.out.push(' ');
                                }
                            } else {
                                self.out.push_str(&rendered);
                            }
                            pos = after;
                        }
                    }
                }
            }
        }
    }

    /// Text living directly in a block container.
    fn push_block_text(&mut self, start: usize, end: usize) {
        if start >= end {
            return;
        }
        let raw = &self.src[start..end];
        let text = decode(raw);

        if text.trim().is_empty() {
            // Keep a single separating space between two inline runs, but never
            // at the beginning of a line or right after a list marker.
            if !self.out.is_empty() && !self.out.ends_with('\n') && !self.out.ends_with(' ') {
                self.out.push(' ');
            }
            return;
        }

        self.push_text(&text);
    }

    /// Appends inline text, collapsing whitespace runs into single spaces.
    fn push_text(&mut self, text: &str) {
        let collapsed = collapse_keep_bounds(text);
        if collapsed.trim().is_empty() {
            return;
        }
        let mut value = collapsed.as_str();
        if value.starts_with(' ')
            && (self.out.is_empty() || self.out.ends_with(' ') || self.out.ends_with('\n'))
        {
            value = value.trim_start();
        }
        self.out.push_str(value);
    }

    fn start_block(&mut self) {
        if self.tight == 0 {
            self.ensure_blank();
        }
    }

    fn end_block(&mut self) {
        if self.tight == 0 {
            self.ensure_blank();
        } else {
            self.ensure_newline();
            let indent = "  ".repeat(self.lists.len());
            self.out.push_str(&indent);
        }
    }

    fn ensure_newline(&mut self) {
        if self.out.is_empty() {
            return;
        }
        while self.out.ends_with(' ') || self.out.ends_with('\t') {
            self.out.pop();
        }
        if !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }

    fn ensure_blank(&mut self) {
        if self.out.is_empty() {
            return;
        }
        while self.out.ends_with(' ') || self.out.ends_with('\t') {
            self.out.pop();
        }
        while !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    // ----------------------------------------------------------------- lists

    fn render_list(&mut self, start: usize, end: usize, ordered: bool) {
        if self.tight > 0 {
            self.ensure_newline();
        } else {
            self.ensure_blank();
        }

        let indent = "  ".repeat(self.lists.len());
        self.lists.push(ordered);

        let src = self.src;
        let mut pos = start;
        let mut index = 1usize;

        while pos < end {
            let tag = match next_tag(src, pos) {
                Some(tag) if tag.start < end && tag.end <= end => tag,
                _ => break,
            };
            pos = tag.end;

            if tag.closing || !tag.name.eq_ignore_ascii_case("li") {
                continue;
            }

            let (inner_end, after) = bounds(src, "li", tag.end, end);
            self.ensure_newline();
            self.out.push_str(&indent);
            if ordered {
                self.out.push_str(&format!("{}. ", index));
                index += 1;
            } else {
                self.out.push_str("- ");
            }

            self.tight += 1;
            self.render_block_range(tag.end, inner_end);
            self.tight -= 1;

            self.ensure_newline();
            pos = after.max(tag.end);
        }

        self.lists.pop();

        if self.tight > 0 {
            self.ensure_newline();
        } else {
            self.ensure_blank();
        }
    }

    // ------------------------------------------------------------- code blocks

    fn render_pre(&mut self, tag: &Tag<'_>, inner_start: usize, inner_end: usize) {
        let src = self.src;
        let mut language = attr(tag, "class")
            .and_then(language_from_class)
            .unwrap_or_default();

        let mut content = &src[inner_start..inner_end];

        // `<pre><code class="language-x">` carries the payload in the child.
        let leading = content.len() - content.trim_start().len();
        if let Some(inner) = next_tag(content, leading) {
            if inner.start == leading
                && !inner.closing
                && inner.name.eq_ignore_ascii_case("code")
                && inner.end <= content.len()
            {
                if language.is_empty() {
                    if let Some(value) = attr(&inner, "class") {
                        language = language_from_class(value).unwrap_or_default();
                    }
                }
                let close = matching_close_start(content, "code", inner.end).min(content.len());
                if close >= inner.end {
                    content = &content[inner.end..close];
                }
            }
        }

        let decoded = decode(content);
        let body = decoded.strip_prefix('\n').unwrap_or(&decoded);
        let body = body.trim_end_matches(['\n', '\r']);

        self.start_block();
        self.out.push_str("```");
        self.out.push_str(&language);
        self.out.push('\n');
        self.out.push_str(body);
        self.out.push('\n');
        self.out.push_str("```");
        self.end_block();
    }

    // ----------------------------------------------------------------- tables

    fn render_table(&mut self, start: usize, end: usize) {
        let src = self.src;
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut pos = start;

        while let Some(tag) = next_tag(src, pos) {
            if tag.start >= end || tag.end > end {
                break;
            }
            pos = tag.end;

            if tag.closing || !tag.name.eq_ignore_ascii_case("tr") {
                continue;
            }

            let (row_end, after) = bounds(src, "tr", tag.end, end);
            let mut cells: Vec<String> = Vec::new();
            let mut cell_pos = tag.end;

            while let Some(cell) = next_tag(src, cell_pos) {
                if cell.start >= row_end || cell.end > row_end {
                    break;
                }
                cell_pos = cell.end;

                if cell.closing
                    || !(cell.name.eq_ignore_ascii_case("td")
                        || cell.name.eq_ignore_ascii_case("th"))
                {
                    continue;
                }

                let (cell_inner_end, cell_after) = bounds(src, cell.name, cell.end, row_end);
                let text = collapse_whitespace(&self.inline(cell.end, cell_inner_end));
                cells.push(text.replace('|', "\\|"));
                cell_pos = cell_after.max(cell.end);
            }

            if !cells.is_empty() {
                rows.push(cells);
            }
            pos = after.max(tag.end);
        }

        if rows.is_empty() {
            return;
        }

        let columns = rows.iter().map(|row| row.len()).max().unwrap_or(0);
        if columns == 0 {
            return;
        }

        self.start_block();

        // The first row always acts as the header: GFM requires one.
        let mut header = rows[0].clone();
        header.resize(columns, String::new());
        self.out.push_str(&table_line(&header));

        let mut separator = String::from("|");
        for _ in 0..columns {
            separator.push_str(" --- |");
        }
        self.out.push('\n');
        self.out.push_str(&separator);

        for row in rows.iter().skip(1) {
            let mut cells = row.clone();
            cells.resize(columns, String::new());
            self.out.push('\n');
            self.out.push_str(&table_line(&cells));
        }

        self.end_block();
    }

    // ------------------------------------------------------------- blockquote

    fn render_quote(&mut self, start: usize, end: usize) {
        let saved_out = std::mem::take(&mut self.out);
        let saved_lists = std::mem::take(&mut self.lists);
        let saved_tight = self.tight;
        self.tight = 0;

        self.render_block_range(start, end);

        let inner = normalize(std::mem::take(&mut self.out));

        self.out = saved_out;
        self.lists = saved_lists;
        self.tight = saved_tight;

        if inner.trim().is_empty() {
            return;
        }

        self.start_block();
        for line in inner.lines() {
            if line.is_empty() {
                self.out.push_str(">\n");
            } else {
                self.out.push_str("> ");
                self.out.push_str(line);
                self.out.push('\n');
            }
        }
        self.end_block();
    }

    // ----------------------------------------------------------------- inline

    fn inline(&mut self, start: usize, end: usize) -> String {
        let src = self.src;
        let mut out = String::new();
        let mut pos = start;

        while pos < end {
            let tag = match next_tag(src, pos) {
                Some(tag) if tag.start < end => tag,
                _ => {
                    out.push_str(&inline_text(&src[pos..end]));
                    break;
                }
            };

            out.push_str(&inline_text(&src[pos..tag.start]));

            if tag.end > end {
                out.push_str(&inline_text(&src[tag.start..end]));
                break;
            }
            pos = tag.end;

            if tag.closing {
                continue;
            }

            let name = tag.name.to_ascii_lowercase();

            match name.as_str() {
                "br" => out.push('\n'),
                "hr" => out.push_str("\n---\n"),
                "wbr" => {}
                "img" => {
                    let source = attr(&tag, "src").map(decode);
                    let alt = attr(&tag, "alt")
                        .map(|value| collapse_whitespace(&decode(value)))
                        .unwrap_or_default();
                    if let Some(source) = source {
                        let url = clean_url(&source);
                        if !is_dangerous_url(&url) {
                            out.push_str("![");
                            out.push_str(&alt);
                            out.push_str("](");
                            out.push_str(&url);
                            out.push(')');
                        }
                    }
                }
                "strong" | "b" => {
                    let (inner_end, after) = bounds(src, &name, tag.end, end);
                    let text = self.inline(tag.end, inner_end);
                    wrap(&mut out, "**", &text);
                    pos = after;
                }
                "em" | "i" | "cite" | "var" | "dfn" => {
                    let (inner_end, after) = bounds(src, &name, tag.end, end);
                    let text = self.inline(tag.end, inner_end);
                    wrap(&mut out, "*", &text);
                    pos = after;
                }
                "del" | "s" | "strike" => {
                    let (inner_end, after) = bounds(src, &name, tag.end, end);
                    let text = self.inline(tag.end, inner_end);
                    wrap(&mut out, "~~", &text);
                    pos = after;
                }
                "code" | "tt" | "kbd" | "samp" => {
                    let (inner_end, after) = bounds(src, &name, tag.end, end);
                    let raw = decode(&src[tag.end..inner_end]);
                    push_inline_code(&mut out, &raw);
                    pos = after;
                }
                "a" => {
                    let (inner_end, after) = bounds(src, &name, tag.end, end);
                    let text = collapse_whitespace(&self.inline(tag.end, inner_end));
                    let href = attr(&tag, "href").map(|value| clean_url(&decode(value)));
                    let title = attr(&tag, "title")
                        .map(|value| collapse_whitespace(&decode(value)))
                        .filter(|value| !value.is_empty());
                    match href {
                        Some(href) if !href.is_empty() && !is_dangerous_url(&href) => {
                            if text.is_empty() {
                                out.push_str(&href);
                            } else {
                                out.push('[');
                                out.push_str(&text);
                                out.push_str("](");
                                out.push_str(&href);
                                if let Some(title) = title {
                                    out.push_str(" \"");
                                    out.push_str(&title);
                                    out.push('"');
                                }
                                out.push(')');
                            }
                        }
                        _ => out.push_str(&text),
                    }
                    pos = after;
                }
                _ => {}
            }
        }

        out
    }
}

// ------------------------------------------------------------------- helpers

/// Byte bounds `(inner_end, after_close)` of the element named `name`.
fn bounds(src: &str, name: &str, from: usize, limit: usize) -> (usize, usize) {
    let inner_end = matching_close_start(src, name, from).min(limit);
    let after = matching_close_end(src, name, from).min(limit);
    (inner_end.max(from), after.max(from))
}

/// End of the element named `name`, or `None` when it is never closed.
fn element_end(src: &str, name: &str, from: usize) -> Option<usize> {
    let close = matching_close_start(src, name, from);
    if close >= src.len() {
        return None;
    }
    Some(matching_close_end(src, name, from))
}

/// Byte index of the closing tag matching an element opened before `from`.
fn matching_close_start(src: &str, name: &str, from: usize) -> usize {
    let mut depth = 1usize;
    let mut pos = from;

    while let Some(tag) = next_tag(src, pos) {
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
        pos = tag.end;
    }

    src.len()
}

/// Byte index just past the closing tag matching an opened element.
fn matching_close_end(src: &str, name: &str, from: usize) -> usize {
    let close = matching_close_start(src, name, from);
    if close >= src.len() {
        return src.len();
    }
    match src[close..].find('>') {
        Some(offset) => close + offset + 1,
        None => src.len(),
    }
}

fn is_block(name: &str) -> bool {
    BLOCK_TAGS
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

/// Decodes entities and collapses whitespace runs, keeping the outer spaces.
fn inline_text(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    collapse_keep_bounds(&decode(raw))
}

fn collapse_keep_bounds(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_space = false;

    for ch in input.chars() {
        if ch.is_whitespace() {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
        } else {
            out.push(ch);
            in_space = false;
        }
    }

    out
}

/// Emits `marker + text + marker`, or nothing when the content is blank.
fn wrap(out: &mut String, marker: &str, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        if !out.is_empty() && !out.ends_with(' ') && !out.ends_with('\n') {
            out.push(' ');
        }
        return;
    }
    out.push_str(marker);
    out.push_str(trimmed);
    out.push_str(marker);
}

/// Emits inline code, choosing a delimiter that cannot collide with the payload.
fn push_inline_code(out: &mut String, raw: &str) {
    let code = collapse_whitespace(raw);
    if code.is_empty() {
        return;
    }
    let fence = if code.contains('`') { "``" } else { "`" };
    out.push_str(fence);
    if code.starts_with('`') || code.ends_with('`') {
        out.push(' ');
        out.push_str(&code);
        out.push(' ');
    } else {
        out.push_str(&code);
    }
    out.push_str(fence);
}

/// Extracts the `language-xxx` / `lang-xxx` token of a `class` attribute.
fn language_from_class(value: &str) -> Option<String> {
    for token in value.split_whitespace() {
        let lower = token.to_ascii_lowercase();
        for prefix in ["language-", "lang-", "highlight-"] {
            if let Some(language) = lower.strip_prefix(prefix) {
                let cleaned: String = language
                    .chars()
                    .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_' || *ch == '+')
                    .collect();
                if !cleaned.is_empty() {
                    return Some(cleaned);
                }
            }
        }
    }
    None
}

fn clean_url(raw: &str) -> String {
    let decoded = decode(raw);
    decoded.trim().trim_matches(|ch| ch == '"' || ch == '\'').to_string()
}

fn is_dangerous_url(url: &str) -> bool {
    let normalized: String = url
        .chars()
        .filter(|ch| !ch.is_whitespace() && !ch.is_control())
        .collect::<String>()
        .to_ascii_lowercase();

    if normalized.is_empty() {
        return true;
    }

    ["javascript:", "vbscript:", "data:", "file:"]
        .iter()
        .any(|scheme| normalized.starts_with(scheme))
}

fn table_line(cells: &[String]) -> String {
    format!("| {} |", cells.join(" | "))
}

/// Final pass: at most one blank line in a row, no trailing spaces, fences
/// preserved byte-for-byte.
fn normalize(markdown: String) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut in_fence = false;

    for line in markdown.lines() {
        let is_fence = line.trim_start().starts_with("```");

        if in_fence {
            lines.push(line.to_string());
            if is_fence {
                in_fence = false;
            }
            continue;
        }

        if is_fence {
            in_fence = true;
            lines.push(line.trim_end().to_string());
            continue;
        }

        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            if lines.is_empty() || lines.last().map(|last| last.is_empty()).unwrap_or(false) {
                continue;
            }
            lines.push(String::new());
        } else {
            lines.push(trimmed.to_string());
        }
    }

    while lines.last().map(|last| last.is_empty()).unwrap_or(false) {
        lines.pop();
    }

    if lines.is_empty() {
        return String::new();
    }

    let mut result = lines.join("\n");
    result.push('\n');
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_h1_to_h6_map_to_hash_levels() {
        let html = "<h1>One</h1><h2>Two</h2><h3>Three</h3><h4>Four</h4><h5>Five</h5><h6>Six</h6>";
        assert_eq!(
            to_markdown(html),
            "# One\n\n## Two\n\n### Three\n\n#### Four\n\n##### Five\n\n###### Six\n"
        );

        // Inline markup inside a heading is converted, not dropped.
        let markdown = to_markdown("<h2>A <em>b</em> &amp; <strong>c</strong></h2>");
        assert_eq!(markdown, "## A *b* & **c**\n");

        // Empty headings produce no output at all.
        assert_eq!(to_markdown("<h3>   </h3>"), "");
    }

    #[test]
    fn paragraphs_and_br_with_no_more_than_two_consecutive_newlines() {
        assert_eq!(
            to_markdown("<p>First paragraph.</p><p>Second<br>line<br/>here.</p>"),
            "First paragraph.\n\nSecond\nline\nhere.\n"
        );

        // Empty paragraphs never accumulate blank lines.
        let markdown = to_markdown("<p>a</p><p></p><p> </p><p>b</p>");
        assert_eq!(markdown, "a\n\nb\n");
        assert!(!markdown.contains("\n\n\n"));

        // Raw newlines between blocks are collapsed as well.
        let markdown = to_markdown("<div><p>x</p>\n\n\n\n<p>y</p></div>");
        assert_eq!(markdown, "x\n\ny\n");
        assert!(!markdown.contains("\n\n\n"));

        // A trailing <br> must not leave dangling blank lines.
        assert_eq!(to_markdown("<p>only<br></p>"), "only\n");
    }

    #[test]
    fn inline_emphasis_and_inline_code_markers() {
        let html = "<p><strong>bold</strong> <b>also bold</b> <em>it</em> <i>also</i> \
                    <code>x = 1</code></p>";
        assert_eq!(
            to_markdown(html),
            "**bold** **also bold** *it* *also* `x = 1`\n"
        );

        // Inline code keeps its payload verbatim apart from entities.
        assert_eq!(
            to_markdown("<p>Use <code>--flag &amp;&amp; value</code> now</p>"),
            "Use `--flag && value` now\n"
        );

        // Nested emphasis is supported.
        assert_eq!(
            to_markdown("<p><strong>outer <em>inner</em></strong></p>"),
            "**outer *inner***\n"
        );

        // Empty emphasis markers are dropped instead of leaking asterisks.
        assert_eq!(to_markdown("<p>a<b> </b>b</p>"), "a b\n");
    }

    #[test]
    fn pre_code_block_preserves_indentation_newlines_and_language() {
        let html = "<pre><code class=\"language-rust\">fn main() {\n    let x = 1 &amp;&amp; 2;\n\tif x {}\n}</code></pre>";
        assert_eq!(
            to_markdown(html),
            "```rust\nfn main() {\n    let x = 1 && 2;\n\tif x {}\n}\n```\n"
        );

        // Language can also live on the <pre> element.
        assert_eq!(
            to_markdown("<pre class=\"lang-python\">x = 1\n  y = 2</pre>"),
            "```python\nx = 1\n  y = 2\n```\n"
        );

        // No language -> bare fence, indentation still exact.
        assert_eq!(
            to_markdown("<pre>\nplain\n    indented\n</pre>"),
            "```\nplain\n    indented\n```\n"
        );

        // An empty block still yields a pair of fences.
        assert_eq!(to_markdown("<pre></pre>"), "```\n\n```\n");
    }

    #[test]
    fn unordered_ordered_and_nested_lists() {
        let html = "<ul><li>Alpha</li><li>Beta<ul><li>Nested</li>\
                    <li>Deep<ol><li>One</li><li>Two</li></ol></li></ul></li></ul>";
        assert_eq!(
            to_markdown(html),
            "- Alpha\n- Beta\n  - Nested\n  - Deep\n    1. One\n    2. Two\n"
        );

        assert_eq!(
            to_markdown("<ol><li>First</li><li>Second</li></ol>"),
            "1. First\n2. Second\n"
        );

        // Paragraphs inside an item stay attached to the bullet.
        assert_eq!(
            to_markdown("<ul><li><p>one</p><p>two</p></li></ul>"),
            "- one\n  two\n"
        );
    }

    #[test]
    fn links_conversion_and_dangerous_href_ignored() {
        let html = "<p>See <a href=\"https://example.com/page\">the docs</a> \
                    and <a href=\"javascript:alert(1)\">click</a>.</p>";
        assert_eq!(
            to_markdown(html),
            "See [the docs](https://example.com/page) and click.\n"
        );

        // Casing and entities in the URL are normalized.
        assert_eq!(
            to_markdown("<p><a href=\"/a?b=1&amp;c=2\">x</a></p>"),
            "[x](/a?b=1&c=2)\n"
        );

        // Titles are kept, anchors without href degrade to plain text.
        assert_eq!(
            to_markdown("<p><a href=\"/x\" title=\"T\">t</a> <a name=\"top\">anchor</a></p>"),
            "[t](/x \"T\") anchor\n"
        );

        for dangerous in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            " java\nscript:alert(1)",
            "data:text/html,<b>x</b>",
            "vbscript:msgbox(1)",
        ] {
            let markdown = to_markdown(&format!("<p><a href=\"{}\">label</a></p>", dangerous));
            assert_eq!(markdown, "label\n", "link leaked for {:?}", dangerous);
            assert!(!markdown.contains("]("));
        }
    }

    #[test]
    fn simple_tables_to_gfm_with_normalized_columns() {
        let html = "<table><thead><tr><th>Name</th><th>Age</th></tr></thead>\
                    <tbody><tr><td>Bob</td><td>4</td></tr>\
                    <tr><td>Alice</td></tr></tbody></table>";
        assert_eq!(
            to_markdown(html),
            "| Name | Age |\n| --- | --- |\n| Bob | 4 |\n| Alice |  |\n"
        );

        // Header-less tables promote the first row, and pipes are escaped.
        let html = "<table><tr><td>a|b</td><td><strong>c</strong></td></tr>\
                    <tr><td>d</td><td>e</td></tr></table>";
        assert_eq!(
            to_markdown(html),
            "| a\\|b | **c** |\n| --- | --- |\n| d | e |\n"
        );

        // Ragged tables are widened to the largest row.
        let html = "<table><tr><th>h</th></tr><tr><td>1</td><td>2</td><td>3</td></tr></table>";
        assert_eq!(
            to_markdown(html),
            "| h |  |  |\n| --- | --- | --- |\n| 1 | 2 | 3 |\n"
        );
    }
}
