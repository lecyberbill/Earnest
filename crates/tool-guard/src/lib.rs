//! tool-guard : validation défensive de chemins, fichiers et paramètres
//! d'outils, sans aucune dépendance externe (contrat C10).

pub mod error;
pub mod file;
pub mod params;
pub mod path;

pub use error::GuardError;