//! Orchestrateur central : applique les règles sur la fenêtre glissante.

use std::collections::VecDeque;

use crate::config::Config;
use crate::rules::{AlternationRule, ExactRepeatRule, Rule, StagnationRule};
use crate::verdict::Verdict;
use crate::ToolCall;

/// Détecteur de boucles : maintient une fenêtre glissante d'appels d'outil
/// et évalue les règles de détection à la demande.
pub struct LoopDetector {
    config: Config,
    window: VecDeque<ToolCall>,
}

impl LoopDetector {
    /// Crée un détecteur avec la configuration donnée.
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            config,
            window: VecDeque::new(),
        }
    }

    /// Pousse un appel d'outil dans la fenêtre (évacue l'élément le plus ancien si nécessaire).
    pub fn observe(&mut self, name: &str, args_hash: u64) {
        self.window.push_back(ToolCall {
            name: name.to_string(),
            args_hash,
        });
        while self.window.len() > self.config.window_size {
            self.window.pop_front();
        }
    }

    /// Réinitialise la fenêtre (après un jalon de progression réussi).
    pub fn reset_progress(&mut self) {
        self.window.clear();
    }

    /// Évalue toutes les règles sur la fenêtre courante et retourne un verdict.
    #[must_use]
    pub fn evaluate(&self) -> Verdict {
        let calls: Vec<ToolCall> = self.window.iter().cloned().collect();
        if calls.is_empty() {
            return Verdict::NoLoop;
        }

        let rules: Vec<Box<dyn Rule>> = vec![
            Box::new(ExactRepeatRule::new(self.config.exact_repeat_threshold)),
            Box::new(StagnationRule::new(self.config.stagnation_threshold)),
            Box::new(AlternationRule::new(self.config.alternation_threshold)),
        ];

        let mut scores: Vec<(u32, String)> = Vec::new();
        for rule in &rules {
            let score = rule.evaluate(&calls);
            if score > 0 {
                scores.push((score, rule.name().to_string()));
            }
        }

        // Tri déterministe : score décroissant, puis nom alphabétique.
        scores.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

        match scores.first() {
            Some((top_score, top_rule)) => {
                if *top_score >= self.config.graduation_threshold {
                    Verdict::LoopDetected {
                        rule: top_rule.clone(),
                        confidence: *top_score,
                    }
                } else {
                    Verdict::Warning {
                        rule: top_rule.clone(),
                        confidence: *top_score,
                    }
                }
            }
            None => Verdict::NoLoop,
        }
    }

    /// Taille actuelle de la fenêtre.
    #[must_use]
    pub fn window_size(&self) -> usize {
        self.window.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> Config {
        Config::default()
    }

    #[test]
    fn reset_progress() {
        let mut det = LoopDetector::new(default_config());
        det.observe("tool_a", 1);
        det.observe("tool_b", 2);
        det.observe("tool_c", 3);
        assert_eq!(det.window_size(), 3);
        det.reset_progress();
        assert_eq!(det.window_size(), 0);
    }

    #[test]
    fn window_bounds() {
        let config = Config::new(5, 3, 8, 2).unwrap();
        let mut det = LoopDetector::new(config);
        for i in 0..10u64 {
            det.observe("tool", i);
        }
        assert_eq!(det.window_size(), 5);
    }

    #[test]
    fn determinism() {
        let config = default_config();
        let mut det1 = LoopDetector::new(config.clone());
        let mut det2 = LoopDetector::new(config);

        let calls = vec![
            ("a", 1u64),
            ("b", 2),
            ("a", 1),
            ("b", 2),
            ("a", 1),
            ("b", 2),
        ];

        for (name, hash) in &calls {
            det1.observe(name, *hash);
            det2.observe(name, *hash);
        }

        let v1 = det1.evaluate();
        let v2 = det2.evaluate();
        assert_eq!(format!("{v1:?}"), format!("{v2:?}"));
    }

    #[test]
    fn no_panic_empty() {
        let det = LoopDetector::new(default_config());
        let verdict = det.evaluate();
        assert!(matches!(verdict, Verdict::NoLoop));
    }

    #[test]
    fn exact_repeat_triggers_loop_detected() {
        let mut det = LoopDetector::new(default_config());
        for _ in 0..5 {
            det.observe("tool", 42);
        }
        let v = det.evaluate();
        assert!(matches!(v, Verdict::LoopDetected { .. }));
    }
}
