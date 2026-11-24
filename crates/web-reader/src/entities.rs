//! Minimal, dependency-free HTML character reference decoder.
//!
//! Supports named entities (see `NAMED_ENTITIES`), decimal references
//! (`&#38;`) and hexadecimal references (`&#x26;`).
//! Anything malformed is restituted literally so that no surrounding
//! information is ever lost, and invalid code points map to U+FFFD.

/// Named character references supported out of the box (all lowercase keys).
const NAMED_ENTITIES: &[(&str, &str)] = &[
    ("amp", "&"),
    ("lt", "<"),
    ("gt", ">"),
    ("quot", "\""),
    ("apos", "'"),
    ("nbsp", " "),
    ("ensp", " "),
    ("emsp", " "),
    ("thinsp", " "),
    ("shy", ""),
    ("copy", "\u{00A9}"),
    ("reg", "\u{00AE}"),
    ("trade", "\u{2122}"),
    ("deg", "\u{00B0}"),
    ("plusmn", "\u{00B1}"),
    ("micro", "\u{00B5}"),
    ("para", "\u{00B6}"),
    ("sect", "\u{00A7}"),
    ("middot", "\u{00B7}"),
    ("bull", "\u{2022}"),
    ("hellip", "\u{2026}"),
    ("prime", "\u{2032}"),
    ("ndash", "\u{2013}"),
    ("mdash", "\u{2014}"),
    ("lsquo", "\u{2018}"),
    ("rsquo", "\u{2019}"),
    ("ldquo", "\u{201C}"),
    ("rdquo", "\u{201D}"),
    ("laquo", "\u{00AB}"),
    ("raquo", "\u{00BB}"),
    ("lsaquo", "\u{2039}"),
    ("rsaquo", "\u{203A}"),
    ("times", "\u{00D7}"),
    ("divide", "\u{00F7}"),
    ("ne", "\u{2260}"),
    ("le", "\u{2264}"),
    ("ge", "\u{2265}"),
    ("asymp", "\u{2248}"),
    ("equiv", "\u{2261}"),
    ("infin", "\u{221E}"),
    ("sum", "\u{2211}"),
    ("prod", "\u{220F}"),
    ("minus", "\u{2212}"),
    ("radic", "\u{221A}"),
    ("larr", "\u{2190}"),
    ("uarr", "\u{2191}"),
    ("rarr", "\u{2192}"),
    ("darr", "\u{2193}"),
    ("harr", "\u{2194}"),
    ("frac12", "\u{00BD}"),
    ("frac14", "\u{00BC}"),
    ("frac34", "\u{00BE}"),
    ("sup1", "\u{00B9}"),
    ("sup2", "\u{00B2}"),
    ("sup3", "\u{00B3}"),
    ("ordf", "\u{00AA}"),
    ("ordm", "\u{00BA}"),
    ("iexcl", "\u{00A1}"),
    ("iquest", "\u{00BF}"),
    ("cent", "\u{00A2}"),
    ("pound", "\u{00A3}"),
    ("curren", "\u{00A4}"),
    ("yen", "\u{00A5}"),
    ("euro", "\u{20AC}"),
    ("brvbar", "\u{00A6}"),
    ("uml", "\u{00A8}"),
    ("acute", "\u{00B4}"),
    ("cedil", "\u{00B8}"),
    ("macr", "\u{00AF}"),
    ("not", "\u{00AC}"),
    ("dagger", "\u{2020}"),
    ("permil", "\u{2030}"),
    ("alpha", "\u{03B1}"),
    ("beta", "\u{03B2}"),
    ("gamma", "\u{03B3}"),
    ("delta", "\u{03B4}"),
    ("epsilon", "\u{03B5}"),
    ("lambda", "\u{03BB}"),
    ("mu", "\u{03BC}"),
    ("pi", "\u{03C0}"),
    ("sigma", "\u{03C3}"),
    ("omega", "\u{03C9}"),
];

/// Maximum number of bytes scanned after `&` while looking for the `;`.
const MAX_REFERENCE_LEN: usize = 40;

/// Decodes every HTML character reference found in `input`.
///
/// Unknown or syntactically broken references are copied verbatim.
pub fn decode(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len());
    let mut index = 0;

    while index < input.len() {
        let byte = input.as_bytes()[index];
        if byte != b'&' {
            let ch = input[index..].chars().next().unwrap_or('\u{FFFD}');
            out.push(ch);
            index += ch.len_utf8();
            continue;
        }

        match decode_reference(&input[index..]) {
            Some((replacement, consumed)) => {
                out.push_str(&replacement);
                index += consumed;
            }
            None => {
                out.push('&');
                index += 1;
            }
        }
    }

    out
}

/// Collapses every whitespace run into a single space and trims both ends.
pub fn collapse_whitespace(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending_space = false;

    for ch in input.chars() {
        if ch.is_whitespace() {
            if !out.is_empty() {
                pending_space = true;
            }
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(ch);
        }
    }

    out
}

/// Decodes a single reference located at the beginning of `input`
/// (which must start with `&`). Returns the replacement text and the
/// number of bytes consumed, including the leading `&` and trailing `;`.
fn decode_reference(input: &str) -> Option<(String, usize)> {
    debug_assert!(input.starts_with('&'));

    let limit = input.len().min(MAX_REFERENCE_LEN);
    let semi = input.get(..limit)?.find(';')?;
    let body = input.get(1..semi)?;
    if body.is_empty() {
        return None;
    }

    if let Some(rest) = body.strip_prefix('#') {
        let (radix, digits) = if let Some(hex) = rest.strip_prefix(['x', 'X']) {
            (16u32, hex)
        } else {
            (10u32, rest)
        };

        if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
            return None;
        }

        let value = u32::from_str_radix(digits, radix).ok()?;
        let ch = match char::from_u32(value) {
            Some(c) if !is_forbidden(c) => c,
            _ => '\u{FFFD}',
        };
        return Some((ch.to_string(), semi + 1));
    }

    if !body.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }

    let key = body.to_ascii_lowercase();
    let found = NAMED_ENTITIES
        .iter()
        .find(|(name, _)| *name == key.as_str())?;

    Some((found.1.to_string(), semi + 1))
}

/// Characters that must never be reintroduced in a text document.
fn is_forbidden(ch: char) -> bool {
    ch.is_control() && !matches!(ch, '\n' | '\r' | '\t')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_entities_named_decimal_hex_and_malformed() {
        assert_eq!(decode("&amp; &lt; &gt; &quot; &apos;"), "& < > \" '");
        assert_eq!(decode("Tom &amp;amp; Jerry"), "Tom &amp; Jerry");
        assert_eq!(decode("&#65;&#x42;&#X43;"), "ABC");
        assert_eq!(decode("&#x1F600;"), "\u{1F600}");
        assert_eq!(decode("caf\u{00E9} &copy;"), "caf\u{00E9} \u{00A9}");
        assert_eq!(decode("&nbsp;"), " ");

        // Malformed references stay literal.
        assert_eq!(decode("&amp"), "&amp");
        assert_eq!(decode("&unknown;"), "&unknown;");
        assert_eq!(decode("&#;"), "&#;");
        assert_eq!(decode("&#xZZ;"), "&#xZZ;");
        assert_eq!(decode("a & b"), "a & b");
        assert_eq!(decode("&;"), "&;");

        // Invalid code points degrade gracefully instead of panicking.
        assert_eq!(decode("&#0;"), "\u{FFFD}");
        assert_eq!(decode("&#x110000;"), "\u{FFFD}");
        assert_eq!(decode("&#99999999999999;"), "&#99999999999999;");
    }

    #[test]
    fn whitespace_collapse_is_stable() {
        assert_eq!(collapse_whitespace("  a \n\t b  "), "a b");
        assert_eq!(collapse_whitespace(""), "");
        assert_eq!(collapse_whitespace("   "), "");
        assert_eq!(collapse_whitespace("a\u{00A0}b"), "a b");
        assert_eq!(collapse_whitespace("   x"), "x");
        assert_eq!(collapse_whitespace("x   "), "x");
    }
}
