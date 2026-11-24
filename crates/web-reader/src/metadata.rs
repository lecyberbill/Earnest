//! Metadata extraction: `<title>`, `<meta name="description">` and the
//! `og:description` fallback.
//!
//! The extractor is a tolerant, single-pass scanner: attribute order, quoting
//! style (`"x"`, `'x'`, `x`) and letter casing never affect the result.

use crate::entities::{collapse_whitespace, decode};
use crate::PageMetadata;

/// A single start or end tag located in a source document.
pub(crate) struct Tag<'a> {
    /// Tag name as written in the source (compare with `eq_ignore_ascii_case`).
    pub name: &'a str,
    /// Raw attribute pairs `(name, raw value)` in source order.
    pub attrs: Vec<(&'a str, String)>,
    /// `true` for `</name>`.
    pub closing: bool,
    /// `true` for `<name/>`.
    pub self_closing: bool,
    /// Byte index of the opening `<`.
    pub start: usize,
    /// Byte index just past the closing `>`.
    pub end: usize,
}

/// Returns the first tag starting at or after `from`.
///
/// Comments (`<!-- ... -->`) and stray `<` characters are skipped, so the
/// returned `Tag` is always a syntactically plausible element boundary.
pub(crate) fn next_tag<'a>(src: &'a str, from: usize) -> Option<Tag<'a>> {
    let bytes = src.as_bytes();
    let mut i = from;

    while i < bytes.len() {
        if bytes[i] == b'<' {
            if let Some(tag) = parse_tag_at(src, i) {
                return Some(tag);
            }
            i += 1;
            continue;
        }
        i += char_len(bytes, i);
    }

    None
}

/// Reads the attribute `name` (case-insensitive) from `tag`.
pub(crate) fn attr<'t>(tag: &'t Tag<'_>, name: &str) -> Option<&'t str> {
    tag.attrs
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Case-insensitive ASCII substring search returning a byte index.
pub(crate) fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let hay = haystack.as_bytes();
    let pin = needle.as_bytes();
    if pin.is_empty() || hay.len() < pin.len() {
        return None;
    }
    (0..=hay.len() - pin.len()).find(|&i| hay[i..i + pin.len()].eq_ignore_ascii_case(pin))
}

/// Extracts `<title>` plus the most relevant description.
///
/// `meta[name=description]` wins over `og:description`; both are read
/// regardless of attribute order, quoting style or casing.
pub fn extract_metadata(html: &str) -> PageMetadata {
    let mut title: Option<String> = None;
    let mut description: Option<String> = None;
    let mut og_description: Option<String> = None;
    let mut pos = 0usize;

    while let Some(tag) = next_tag(html, pos) {
        pos = tag.end;
        if tag.closing {
            continue;
        }

        if tag.name.eq_ignore_ascii_case("title") {
            if title.is_none() {
                let body_start = tag.end;
                let (raw, next_pos) = match find_ci(&html[body_start..], "</title") {
                    Some(rel) => {
                        let close = body_start + rel;
                        let after = match html[close..].find('>') {
                            Some(gt) => close + gt + 1,
                            None => html.len(),
                        };
                        (&html[body_start..close], after)
                    }
                    None => (&html[body_start..], html.len()),
                };
                let text = collapse_whitespace(&decode(raw));
                if !text.is_empty() {
                    title = Some(text);
                }
                pos = next_pos;
            }
            continue;
        }

        if tag.name.eq_ignore_ascii_case("meta") {
            let key = attr(&tag, "name").or_else(|| attr(&tag, "property"));
            let content = match attr(&tag, "content") {
                Some(value) => collapse_whitespace(&decode(value)),
                None => continue,
            };
            if content.is_empty() {
                continue;
            }
            if let Some(key) = key {
                if key.eq_ignore_ascii_case("description") {
                    if description.is_none() {
                        description = Some(content);
                    }
                } else if key.eq_ignore_ascii_case("og:description") && og_description.is_none() {
                    og_description = Some(content);
                }
            }
        }
    }

    PageMetadata {
        title,
        description: description.or(og_description),
    }
}

/// Byte length of the UTF-8 sequence starting at `i`.
fn char_len(bytes: &[u8], i: usize) -> usize {
    let byte = bytes[i];
    if byte < 0x80 {
        1
    } else if byte >> 5 == 0b110 {
        2
    } else if byte >> 4 == 0b1110 {
        3
    } else if byte >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

/// Parses the tag starting exactly at `start` (which must be a `<`).
fn parse_tag_at(src: &str, start: usize) -> Option<Tag<'_>> {
    let bytes = src.as_bytes();
    if src[start..].starts_with("<!--") {
        return None;
    }

    let mut i = start + 1;
    if i >= bytes.len() {
        return None;
    }

    let closing = bytes[i] == b'/';
    if closing {
        i += 1;
    }

    let name_start = i;
    while i < bytes.len()
        && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'-' | b':' | b'_'))
    {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name = &src[name_start..i];

    if closing {
        let gt = src[i..].find('>')?;
        return Some(Tag {
            name,
            attrs: Vec::new(),
            closing: true,
            self_closing: false,
            start,
            end: i + gt + 1,
        });
    }

    let parsed = parse_attrs(src, i)?;
    Some(Tag {
        name,
        attrs: parsed.attrs,
        closing: false,
        self_closing: parsed.self_closing,
        start,
        end: parsed.end,
    })
}

/// Result of [`parse_attrs`]: attributes, self-closing flag, tag end offset.
struct AttrList<'a> {
    /// Raw `(name, value)` pairs in source order.
    attrs: Vec<(&'a str, String)>,
    /// `true` when the tag was written as `<name/>`.
    self_closing: bool,
    /// Byte index just past the closing `>`.
    end: usize,
}

/// Parses the attribute list of a start tag, returning the closing position.
fn parse_attrs<'a>(src: &'a str, mut i: usize) -> Option<AttrList<'a>> {
    let bytes = src.as_bytes();
    let mut attrs: Vec<(&'a str, String)> = Vec::new();

    loop {
        while i < bytes.len() && is_ascii_space(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        if bytes[i] == b'>' {
            return Some(AttrList {
                attrs,
                self_closing: false,
                end: i + 1,
            });
        }
        if bytes[i] == b'/' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'>' {
                return Some(AttrList {
                    attrs,
                    self_closing: true,
                    end: i + 2,
                });
            }
            i += 1;
            continue;
        }

        let name_start = i;
        while i < bytes.len()
            && !matches!(bytes[i], b'=' | b'>' | b'/' | b'"' | b'\'' ) && !is_ascii_space(bytes[i])
        {
            i += 1;
        }
        if i == name_start {
            return None;
        }
        let name = &src[name_start..i];

        while i < bytes.len() && is_ascii_space(bytes[i]) {
            i += 1;
        }

        let mut value = String::new();
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && is_ascii_space(bytes[i]) {
                i += 1;
            }
            if i >= bytes.len() {
                return None;
            }
            if bytes[i] == b'"' || bytes[i] == b'\'' {
                let quote = bytes[i];
                i += 1;
                let value_start = i;
                while i < bytes.len() && bytes[i] != quote {
                    i += char_len(bytes, i);
                }
                if i >= bytes.len() {
                    return None;
                }
                value = src[value_start..i].to_string();
                i += 1;
            } else {
                let value_start = i;
                while i < bytes.len() && !matches!(bytes[i], b'>' | b'"' | b'\'') && !is_ascii_space(bytes[i])
                {
                    i += 1;
                }
                value = src[value_start..i].to_string();
            }
        }

        attrs.push((name, value));
    }
}

fn is_ascii_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_title_and_description_attribute_order_and_case() {
        // Plain, canonical form.
        let page = extract_metadata(
            "<html><head><title> Hello  World </title><meta name=\"description\" content=\"A page.\"></head></html>",
        );
        assert_eq!(page.title.as_deref(), Some("Hello World"));
        assert_eq!(page.description.as_deref(), Some("A page."));

        // Attribute order, casing, quotes and unquoted values.
        let page = extract_metadata(
            "<HEAD><TITLE>Upper &amp; Case</TITLE><META CONTENT='Desc first' NAME='DESCRIPTION'></HEAD>",
        );
        assert_eq!(page.title.as_deref(), Some("Upper & Case"));
        assert_eq!(page.description.as_deref(), Some("Desc first"));

        let page = extract_metadata(
            "<title lang=en data-x='1'>Unquoted</title><meta content=unquoted-value name=description>",
        );
        assert_eq!(page.title.as_deref(), Some("Unquoted"));
        assert_eq!(page.description.as_deref(), Some("unquoted-value"));

        // og:description fallback, also with reversed attribute order.
        let page = extract_metadata(
            "<meta property=\"og:description\" content=\"Open Graph text\"><title>OG</title>",
        );
        assert_eq!(page.description.as_deref(), Some("Open Graph text"));

        // name=description wins over og:description.
        let page = extract_metadata(
            "<meta property='og:description' content='OG'><meta name='description' content='Meta'>",
        );
        assert_eq!(page.description.as_deref(), Some("Meta"));

        // Entities, multi-line titles and absent metadata.
        let page = extract_metadata("<title>Line\n  one &nbsp;&#65;</title>");
        assert_eq!(page.title.as_deref(), Some("Line one A"));

        let page = extract_metadata("<p>No head here</p>");
        assert_eq!(page, PageMetadata::default());

        // Unclosed title degrades gracefully.
        let page = extract_metadata("<title>Truncated");
        assert_eq!(page.title.as_deref(), Some("Truncated"));
    }
}
