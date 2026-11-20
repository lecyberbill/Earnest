//! Règle de stagnation : N appels identiques consécutifs sans changement de stratégie.

use crate::rules::Rule;
use crate::ToolCall;

/// Détecte la stagnation : `threshold` appels consécutifs strictement identiques.
pub struct StagnationRule {
    threshold: usize,
}

impl StagnationRule {
    /// Crée la règle avec un seuil de stagnation.
    #[must_use]
    pub fn new(threshold: usize) -> Self {
        Self { threshold }
    }
}

impl Rule for StagnationRule {
    fn name(&self) -> &str {
        "stagnation"
    }

    fn evaluate(&self, calls: &[ToolCall]) -> u32 {
        if calls.len() < self.threshold {
            return 0;
        }
        let first = &calls[calls.len() - self.threshold];
        let mut count = 0usize;
        for c in calls.iter().rev() {
            if *c == *first {
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

    fn call(name: &str, h: u64) -> ToolCall {
        ToolCall { name: name.into(), args_hash: h }
    }

    #[test]
    fn stagnation_detected() {
        let r = StagnationRule::new(3);
        let calls = vec![call("a", 1), call("a", 1), call("a", 1)];
        assert_eq!(r.evaluate(&calls), 3);
    }

    #[test]
    fn stagnation_not_detected() {
        let r = StagnationRule::new(3);
        let calls = vec![call("a", 1), call("a", 1), call("b", 2)];
        assert_eq!(r.evaluate(&calls), 0);
    }

    #[test]
    fn stagnation_insufficient_history() {
        let r = StagnationRule::new(5);
        let calls = vec![call("a", 1), call("a", 1), call("a", 1)];
        assert_eq!(r.evaluate(&calls), 0);
    }

    #[test]
    fn stagnation_empty() {
        let r = StagnationRule::new(3);
        assert_eq!(r.evaluate(&[]), 0);
    }
}
