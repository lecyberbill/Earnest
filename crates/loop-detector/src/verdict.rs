//! Verdicts et graduation de sévérité.

use std::fmt;

/// Niveau de sévérité.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Info,
    Warning,
    Critical,
    Fatal,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Info => write!(f, "INFO"),
            Severity::Warning => write!(f, "WARNING"),
            Severity::Critical => write!(f, "CRITICAL"),
            Severity::Fatal => write!(f, "FATAL"),
        }
    }
}

/// Résultat de l'évaluation du détecteur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Aucune boucle détectée.
    NoLoop,
    /// Signal préventif (sous le seuil de blocage).
    Warning { rule: String, confidence: u32 },
    /// Boucle confirmée — blocage recommandé.
    LoopDetected { rule: String, confidence: u32 },
}

impl Verdict {
    /// Sévérité associée à ce verdict.
    #[must_use]
    pub fn severity(&self) -> Severity {
        match self {
            Verdict::NoLoop => Severity::Info,
            Verdict::Warning { .. } => Severity::Warning,
            Verdict::LoopDetected { .. } => Severity::Critical,
        }
    }

    /// Nom de la règle responsable (ou "none").
    #[must_use]
    pub fn rule_name(&self) -> &str {
        match self {
            Verdict::NoLoop => "none",
            Verdict::Warning { rule, .. } | Verdict::LoopDetected { rule, .. } => rule,
        }
    }
}

/// Graduation : retourne la sévérité maximale parmi une liste de verdicts.
pub fn graduate(verdicts: &[Verdict]) -> Severity {
    verdicts.iter().map(|v| v.severity()).max().unwrap_or(Severity::Info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_graduation_empty() {
        assert_eq!(graduate(&[]), Severity::Info);
    }

    #[test]
    fn verdict_graduation_mixed() {
        let vs = vec![
            Verdict::Warning { rule: "r1".into(), confidence: 5 },
            Verdict::LoopDetected { rule: "r2".into(), confidence: 90 },
            Verdict::NoLoop,
        ];
        assert_eq!(graduate(&vs), Severity::Critical);
    }

    #[test]
    fn verdict_graduation_single_warning() {
        let vs = vec![Verdict::Warning { rule: "x".into(), confidence: 3 }];
        assert_eq!(graduate(&vs), Severity::Warning);
    }

    #[test]
    fn verdict_rule_name() {
        assert_eq!(Verdict::NoLoop.rule_name(), "none");
        assert_eq!(
            Verdict::LoopDetected { rule: "exact_repeat".into(), confidence: 10 }.rule_name(),
            "exact_repeat"
        );
    }
}
