//! Règles de détection de boucles.

pub mod alternation;
pub mod exact_repeat;
pub mod stagnation;

pub use alternation::AlternationRule;
pub use exact_repeat::ExactRepeatRule;
pub use stagnation::StagnationRule;

use crate::ToolCall;

/// Trait commun à toutes les règles de détection.
pub trait Rule {
    /// Nom stable de la règle (pour le rapport).
    fn name(&self) -> &str;
    /// Évalue la fenêtre et retourne un score (0 = pas de signal).
    fn evaluate(&self, calls: &[ToolCall]) -> u32;
}
