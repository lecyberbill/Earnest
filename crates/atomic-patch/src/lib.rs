//! # atomic-patch
//!
//! Moteur de patch atomique multi-fichiers avec :
//! - Matcher contextuel (exact > fuzzy > not-found)
//! - Génération de diff unifié
//! - Transaction avec rollback atomique
//!
//! Zéro dépendance externe.

pub mod diff;
pub mod matcher;
pub mod transaction;

// Re-exports de l'API publique
pub use diff::{Diff, DiffLine, DiffLineKind, generate_unified_diff};
pub use matcher::{MatchResult, MatchStatus, Position, match_block};
pub use transaction::{PatchOperation, Transaction, TransactionOutcome};
