//! Shannon entropy helper used by the generic secret heuristic.

use std::collections::HashMap;

/// Computes the Shannon entropy of `text` in **bits per character**.
///
/// The computation iterates over Unicode scalar values (`char`), so the result
/// is well defined for any UTF-8 input and never panics. An empty string has an
/// entropy of `0.0`.
///
/// # Examples
///
/// ```
/// use dotenv_shield::shannon_entropy;
///
/// assert_eq!(shannon_entropy(""), 0.0);
/// assert_eq!(shannon_entropy("aaaa"), 0.0);
/// // A perfectly balanced two-symbol alphabet yields exactly 1 bit per char.
/// assert!((shannon_entropy("abab") - 1.0).abs() < 1e-9);
/// // Random-looking material scores far above the 3.8 bits/char threshold.
/// assert!(shannon_entropy("a8Fk2LmQ9zXr4TbV7nPw") > 3.8);
/// ```
#[must_use]
pub fn shannon_entropy(text: &str) -> f64 {
    let mut counts: HashMap<char, usize> = HashMap::new();
    let mut total: usize = 0;

    for ch in text.chars() {
        *counts.entry(ch).or_insert(0) += 1;
        total += 1;
    }

    if total == 0 {
        return 0.0;
    }

    let total_f = total as f64;
    let mut entropy = 0.0_f64;
    for count in counts.values() {
        let p = *count as f64 / total_f;
        entropy -= p * p.log2();
    }

    // Guard against `-0.0` and NaN leaking out of the accumulation.
    if entropy.is_finite() && entropy > 0.0 {
        entropy
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::shannon_entropy;

    #[test]
    fn empty_and_uniform_inputs() {
        assert_eq!(shannon_entropy(""), 0.0);
        assert_eq!(shannon_entropy("aaaaaaaaaa"), 0.0);
    }

    #[test]
    fn known_values() {
        assert!((shannon_entropy("abab") - 1.0).abs() < 1e-9);
        assert!((shannon_entropy("abcd") - 2.0).abs() < 1e-9);
    }

    #[test]
    fn unicode_is_supported() {
        // Multi-byte characters must not panic and must be counted per scalar.
        let value = shannon_entropy("héllo—wörld—日本語");
        assert!(value > 0.0);
        assert!(value.is_finite());
    }

    #[test]
    fn high_entropy_material() {
        assert!(shannon_entropy("a8Fk2LmQ9zXr4TbV7nPw") > 3.8);
    }
}
