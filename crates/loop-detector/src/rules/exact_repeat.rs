//! Règle de répétition exacte : N appels identiques consécutifs.

use crate::rules::Rule;
use crate::ToolCall;

/// Détecte les séquences de `threshold` appels d'outil identiques consécutifs.
pub struct ExactRepeatRule {
    threshold: usize,
}

impl ExactRepeatRule {
    /// Crée la règle avec un seuil minimal de répétitions.
    #[must_use]
    pub fn new(threshold: usize) -> Self {
        Self { threshold }
    }
}

impl Rule for ExactRepeatRule {
    fn name(&self) -> &str {
        "exact_repeat"
    }

    fn evaluate(&self, calls: &[ToolCall]) -> u32 {
        if calls.is_empty() {
            return 0;
        }
        let last = &calls[calls.len() - 1];
        let mut count = 1usize;
        for c in calls.iter().rev().skip(1) {
            if c == last {
                count += 1;
            } else {
                break;
            }
        }
        if count >= self.threshold {
            count as u32
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> ExactRepeatRule {
        ExactRepeatRule::new(3)
    }

    fn call(name: &str, h: u64) -> ToolCall {
        ToolCall { name: name.into(), args_hash: h }
    }

    #[test]
    fn exact_repeat_detected() {
        let calls = vec![call("a", 1), call("a", 1), call("a", 1)];
        assert_eq!(rule().evaluate(&calls), 3);
    }

    #[test]
    fn exact_repeat_not_below_threshold() {
        let calls = vec![call("a", 1), call("a", 1)];
        assert_eq!(rule().evaluate(&calls), 0);
    }

    #[test]
    fn exact_repeat_mixed() {
        let calls = vec![
            call("x", 9),
            call("a", 1),
            call("a", 1),
            call("a", 1),
        ];
        assert_eq!(rule().evaluate(&calls), 3);
    }

    #[test]
    fn exact_repeat_empty() {
        assert_eq!(rule().evaluate(&[]), 0);
    }

    #[test]
    fn exact_repeat_all_different() {
        let calls = vec![call("a", 1), call("b", 2), call("c", 3)];
        assert_eq!(rule().evaluate(&calls), 0);
    }
}
