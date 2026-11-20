//! Loop-detector: détecte les boucles d'agent via des règles sur une fenêtre glissante.

pub mod config;
pub mod detector;
pub mod rules;
pub mod verdict;

pub use config::{Config, ConfigError};
pub use detector::LoopDetector;
pub use verdict::{Verdict, Severity};

/// Représente un appel d'outil observé depuis l'agent.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolCall {
    /// Nom de l'outil appelé.
    pub name: String,
    /// Empreinte déterministe des arguments.
    pub args_hash: u64,
}

#[cfg(test)]
mod tests {
    /// C9 : aucune dépendance externe autorisée.
    #[test]
    fn no_external_dependencies() {
        let manifest = std::fs::read_to_string("Cargo.toml")
            .expect("Cargo.toml must exist at crate root");
        assert!(
            !manifest.contains("[dependencies]"),
            "loop-detector must not declare external [dependencies]"
        );
        assert!(
            !manifest.contains("[dev-dependencies]"),
            "loop-detector must not declare [dev-dependencies]"
        );
    }
}
