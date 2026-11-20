//! Règle d'alternance cyclique : détecte les patterns A-B-A-B… ou A-B-C-A-B-C…

use crate::rules::Rule;
use crate::ToolCall;

/// Détecte les alternances cycliques de période 2 ou 3.
pub struct AlternationRule {
    min_period: usize,
}

impl AlternationRule {
    /// Crée la règle ; `min_period` est la période minimale à considérer (2 ou 3).
    #[must_use]
    pub fn new(min_period: usize) -> Self {
        Self { min_period: min_period.max(2) }
    }
}

impl Rule for AlternationRule {
    fn name(&self) -> &str {
        "alternation_cycle"
    }

    fn evaluate(&self, calls: &[ToolCall]) -> u32 {
        if calls.len() < 4 {
            return 0;
        }

        let mut best_score: u32 = 0;

        for period in self.min_period..=3usize {
            if calls.len() < period * 2 {
                continue;
            }
            let n = calls.len();
            let checked = n - period;
            let mut matches = 0usize;
            for i in 0..checked {
                if calls[i] == calls[i + period] {
                    matches += 1;
                }
            }
            if checked > 0 {
                let score = (matches as u32) * 100 / checked as u32;
                if score > best_score {
                    best_score = score;
                }
            }
        }

        if best_score >= 85 {
            best_score
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str) -> ToolCall {
        ToolCall { name: name.into(), args_hash: 0 }
    }

    fn rule() -> AlternationRule {
        AlternationRule::new(2)
    }

    #[test]
    fn alternation_cycle_ab() {
        let calls: Vec<ToolCall> = (0..8).map(|i| if i % 2 == 0 { call("read") } else { call("write") }).collect();
        assert!(rule().evaluate(&calls) >= 80);
    }

    #[test]
    fn alternation_cycle_abc() {
        let actions = ["read", "think", "write"];
        let calls: Vec<ToolCall> = (0..9).map(|i| call(actions[i % 3])).collect();
        assert!(rule().evaluate(&calls) >= 80);
    }

    #[test]
    fn no_alternation_random() {
        let calls = vec![
            call("read"),
            call("think"),
            call("read"),
            call("write"),
            call("think"),
            call("read"),
            call("write"),
            call("think"),
        ];
        assert_eq!(rule().evaluate(&calls), 0);
    }

    #[test]
    fn alternation_too_short() {
        let calls = vec![call("a"), call("b")];
        assert_eq!(rule().evaluate(&calls), 0);
    }

    #[test]
    fn alternation_empty() {
        assert_eq!(rule().evaluate(&[]), 0);
    }
}
