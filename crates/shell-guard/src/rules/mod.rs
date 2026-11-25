//! Regles d'analyse statique appliquees aux segments normalises.
//!
//! - `dangerous` : blocage des operations critiques (contrat C5)
//! - `interactive` : blocage des commandes qui attendent un humain (contrats C6 et C7)
//! - `classify` : niveau de risque des commandes autorisees (contrat C8)

pub mod classify;
pub mod dangerous;
pub mod interactive;

/// Vrai si l'un des `needles` correspond a un argument.
///
/// Un drapeau long (`--message`) correspond aussi a sa forme `--message=valeur`
/// et un drapeau court (`-m`) a sa forme collee (`-mtexte`).
pub(crate) fn arg_present(args: &[String], needles: &[&str]) -> bool {
    for a in args {
        let al = a.to_ascii_lowercase();
        for n in needles {
            if al == *n {
                return true;
            }
            if let Some(rest) = al.strip_prefix(*n) {
                if n.starts_with("--") {
                    if rest.starts_with('=') {
                        return true;
                    }
                } else if n.len() == 2 && !rest.is_empty() && !al.starts_with("--") {
                    return true;
                }
            }
        }
    }
    false
}
