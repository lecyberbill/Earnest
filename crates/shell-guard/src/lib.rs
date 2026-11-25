//! `shell_guard` : analyse statique d'une ligne de commande shell avant
//! execution par un agent IA.
//!
//! Le crate est volontairement autonome : edition 2021, aucune dependance
//! externe, uniquement `std`. Il ne lance jamais la commande analysee.
//!
//! Le pipeline est le suivant :
//!
//! 1. `segment::split_segments` decoupe la ligne en commandes simples en
//!    respectant quotes, echappements et sous-commandes `$( ... )` ;
//! 2. `normalize::normalize` identifie la commande reelle en traversant les
//!    prefixes (`sudo`, `env`, `timeout`...) et les shells embarques
//!    (`bash -c`, `powershell -Command`, `cmd /c`) ;
//! 3. `rules::dangerous` bloque les operations critiques irreversibles ;
//! 4. `rules::interactive` bloque les commandes qui attendent un humain ;
//! 5. `rules::classify` qualifie le risque des commandes autorisees.
//!
//! Priorite des verdicts : un danger critique l'emporte sur un blocage
//! interactif, ou qu'il se trouve dans la ligne. A defaut de danger, le
//! premier segment bloquant pour cause d'interactivite est retenu.
//!
//! ```
//! use shell_guard::{inspect_command, RiskLevel, Verdict};
//!
//! assert_eq!(
//!     inspect_command("ls -la"),
//!     Verdict::Allowed { risk: RiskLevel::ReadOnly }
//! );
//! assert!(inspect_command("vim notes.txt").is_blocked());
//! assert!(matches!(
//!     inspect_command("rm -rf /"),
//!     Verdict::DangerousBlocked { .. }
//! ));
//! ```

#![forbid(unsafe_code)]

pub mod normalize;
pub mod rules;
pub mod segment;
pub mod session;

pub use session::{
    allow_for_session, clear_session_overrides, inspect_command_in_workspace,
    normalize_workspace_key, AllowScope,
};

use std::cmp;

use normalize::normalize;
use segment::{split_segments, Segment};

/// Niveau de risque d'une commande autorisee.
///
/// L'ordre est significatif : `ReadOnly < Mutating`, ce qui permet d'agreger
/// plusieurs segments en retenant le risque maximal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RiskLevel {
    /// La commande ne fait que lire des donnees et n'ecrit rien sur le disque.
    ReadOnly,
    /// La commande peut modifier l'etat local ou distant.
    Mutating,
}

impl RiskLevel {
    /// Vrai si la commande est susceptible de modifier l'etat du systeme.
    pub fn is_mutating(self) -> bool {
        matches!(self, RiskLevel::Mutating)
    }

    /// Risque le plus eleve des deux.
    pub fn or(self, other: RiskLevel) -> RiskLevel {
        cmp::max(self, other)
    }
}

/// Verdict rendu par [`inspect_command`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// La commande est sure a executer ; `risk` qualifie son effet de bord.
    Allowed {
        /// Effet de bord le plus eleve observe dans la ligne.
        risk: RiskLevel,
    },
    /// La commande attend une intervention humaine : elle ne rendrait pas la main.
    InteractiveBlocked {
        /// Commande normalisee mise en cause.
        command: String,
        /// Pourquoi la commande bloquerait.
        reason: String,
        /// Equivalent non interactif a utiliser a la place.
        suggestion: String,
    },
    /// La commande est destructrice et irreversible : elle ne doit pas partir.
    DangerousBlocked {
        /// Commande normalisee mise en cause.
        command: String,
        /// Pourquoi la commande est inacceptable.
        reason: String,
    },
}

impl Verdict {
    /// Vrai si la commande peut etre executee.
    pub fn is_allowed(&self) -> bool {
        matches!(self, Verdict::Allowed { .. })
    }

    /// Vrai si la commande est bloquee, quelle qu'en soit la cause.
    pub fn is_blocked(&self) -> bool {
        !self.is_allowed()
    }

    /// Niveau de risque, uniquement pour un verdict [`Verdict::Allowed`].
    pub fn risk(&self) -> Option<RiskLevel> {
        match self {
            Verdict::Allowed { risk } => Some(*risk),
            _ => None,
        }
    }

    /// Nom de commande mis en cause, uniquement pour un verdict bloquant.
    pub fn command(&self) -> Option<&str> {
        match self {
            Verdict::Allowed { .. } => None,
            Verdict::InteractiveBlocked { command, .. } => Some(command.as_str()),
            Verdict::DangerousBlocked { command, .. } => Some(command.as_str()),
        }
    }

    /// Suggestion de remplacement, uniquement pour un blocage interactif.
    pub fn suggestion(&self) -> Option<&str> {
        match self {
            Verdict::InteractiveBlocked { suggestion, .. } => Some(suggestion.as_str()),
            _ => None,
        }
    }
}

/// Analyse statiquement une ligne de commande shell.
///
/// Accepte indifferemment une syntaxe POSIX sh/bash ou Windows
/// PowerShell/cmd. La fonction ne panique jamais : une entree malformee
/// (quote non fermee, sous-commande tronquee, unicode arbitraire) produit un
/// verdict ordinaire.
pub fn inspect_command(cmd: &str) -> Verdict {
    // 1. Motifs reconnaissables seulement sur la ligne entiere (les operateurs
    //    du shell les decoupent en plusieurs segments).
    if let Some(danger) = rules::dangerous::scan_whole_line(cmd) {
        return Verdict::DangerousBlocked {
            command: danger.command,
            reason: danger.reason,
        };
    }

    let segments = split_segments(cmd);

    // 2. Un danger critique l'emporte, ou qu'il soit place.
    for seg in &segments {
        if let Some(danger) = danger_of(seg) {
            return Verdict::DangerousBlocked {
                command: danger.command,
                reason: danger.reason,
            };
        }
    }

    // 3. Sinon, le premier segment interactif bloquant donne le verdict.
    for seg in &segments {
        if let Some(interaction) = interaction_of(seg) {
            return Verdict::InteractiveBlocked {
                command: interaction.command,
                reason: interaction.reason,
                suggestion: interaction.suggestion,
            };
        }
    }

    // 4. Reste : on agrege le risque de tous les segments.
    let mut risk = RiskLevel::ReadOnly;
    for seg in &segments {
        risk = risk.or(risk_of(seg));
    }
    Verdict::Allowed { risk }
}

/// Danger critique du segment, sous-commandes `$( ... )` incluses.
fn danger_of(seg: &Segment) -> Option<rules::dangerous::Danger> {
    let norm = normalize(&seg.raw);
    if let Some(danger) = rules::dangerous::check(seg, &norm) {
        return Some(danger);
    }
    for sub in seg.all_subshells() {
        let sub_norm = normalize(&sub.raw);
        if let Some(danger) = rules::dangerous::check(sub, &sub_norm) {
            return Some(danger);
        }
    }
    None
}

/// Blocage interactif du segment, sous-commandes `$( ... )` incluses.
fn interaction_of(seg: &Segment) -> Option<rules::interactive::Interaction> {
    let norm = normalize(&seg.raw);
    if let Some(interaction) = rules::interactive::check(seg, &norm) {
        return Some(interaction);
    }
    for sub in seg.all_subshells() {
        let sub_norm = normalize(&sub.raw);
        if let Some(interaction) = rules::interactive::check(sub, &sub_norm) {
            return Some(interaction);
        }
    }
    None
}

/// Risque maximal du segment et de ses sous-commandes.
fn risk_of(seg: &Segment) -> RiskLevel {
    let norm = normalize(&seg.raw);
    let mut risk = rules::classify::classify(seg, &norm);
    for sub in seg.all_subshells() {
        let sub_norm = normalize(&sub.raw);
        risk = risk.or(rules::classify::classify(sub, &sub_norm));
    }
    risk
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_publique_et_derives() {
        // --- verdict autorise ---
        let allowed = inspect_command("ls -la");
        assert!(allowed.is_allowed());
        assert!(!allowed.is_blocked());
        assert_eq!(allowed.risk(), Some(RiskLevel::ReadOnly));
        assert_eq!(allowed.command(), None);
        assert_eq!(allowed.suggestion(), None);

        // --- derives Debug / Clone / PartialEq / Eq ---
        assert_eq!(allowed.clone(), allowed);
        assert_ne!(allowed, inspect_command("rm -rf ./build"));
        assert!(format!("{:?}", allowed).contains("Allowed"));
        assert!(format!("{:?}", RiskLevel::ReadOnly).contains("ReadOnly"));
        assert!(format!("{:?}", RiskLevel::Mutating).contains("Mutating"));
        assert_ne!(RiskLevel::ReadOnly, RiskLevel::Mutating);
        assert!(RiskLevel::ReadOnly < RiskLevel::Mutating);
        assert!(RiskLevel::Mutating.is_mutating());
        assert!(!RiskLevel::ReadOnly.is_mutating());
        assert_eq!(RiskLevel::ReadOnly.or(RiskLevel::Mutating), RiskLevel::Mutating);

        // --- verdict interactif : acces a tous les champs ---
        match inspect_command("vim notes.txt") {
            Verdict::InteractiveBlocked {
                command,
                reason,
                suggestion,
            } => {
                assert_eq!(command, "vim");
                assert!(!reason.is_empty());
                assert!(!suggestion.is_empty());
                assert_eq!(Verdict::InteractiveBlocked { command, reason, suggestion }, inspect_command("vim notes.txt"));
            }
            other => panic!("attendu InteractiveBlocked, obtenu {other:?}"),
        }
        let blocked = inspect_command("git commit");
        assert!(blocked.is_blocked());
        assert_eq!(blocked.command(), Some("git"));
        assert!(blocked.suggestion().is_some());
        assert_eq!(blocked.risk(), None);

        // --- verdict dangereux ---
        match inspect_command("rm -rf /") {
            Verdict::DangerousBlocked { command, reason } => {
                assert_eq!(command, "rm");
                assert!(!reason.is_empty());
            }
            other => panic!("attendu DangerousBlocked, obtenu {other:?}"),
        }
        let danger = inspect_command("shutdown -h now");
        assert!(danger.is_blocked());
        assert_eq!(danger.command(), Some("shutdown"));
        assert_eq!(danger.suggestion(), None);
        assert_eq!(danger.risk(), None);

        // --- API de decoupage et de normalisation exposee ---
        assert_eq!(segment::split_segments("ls; pwd").len(), 2);
        assert_eq!(normalize::normalize("sudo rm -rf x").command, "rm");
    }

    #[test]
    fn test_priorite_danger_sur_interactif_et_premier_segment() {
        // Le danger critique l'emporte meme s'il survient apres un blocage
        // interactif.
        match inspect_command("vim notes.txt; rm -rf /") {
            Verdict::DangerousBlocked { command, .. } => assert_eq!(command, "rm"),
            other => panic!("attendu DangerousBlocked, obtenu {other:?}"),
        }
        match inspect_command("less README.md && mkfs.ext4 /dev/sda1") {
            Verdict::DangerousBlocked { command, .. } => assert_eq!(command, "mkfs.ext4"),
            other => panic!("attendu DangerousBlocked, obtenu {other:?}"),
        }

        // Aucun danger : c'est le premier segment fautif qui est retenu.
        match inspect_command("vim a.txt; less b.txt") {
            Verdict::InteractiveBlocked { command, .. } => assert_eq!(command, "vim"),
            other => panic!("attendu InteractiveBlocked, obtenu {other:?}"),
        }
        match inspect_command("ls -la; node") {
            Verdict::InteractiveBlocked { command, .. } => assert_eq!(command, "node"),
            other => panic!("attendu InteractiveBlocked, obtenu {other:?}"),
        }
        match inspect_command("echo ok | less") {
            Verdict::InteractiveBlocked { command, .. } => assert_eq!(command, "less"),
            other => panic!("attendu InteractiveBlocked, obtenu {other:?}"),
        }

        // Un danger cache dans une sous-commande est vu.
        match inspect_command("echo $(rm -rf /)") {
            Verdict::DangerousBlocked { command, .. } => assert_eq!(command, "rm"),
            other => panic!("attendu DangerousBlocked, obtenu {other:?}"),
        }
        match inspect_command("echo $(echo $(shutdown -h now))") {
            Verdict::DangerousBlocked { command, .. } => assert_eq!(command, "shutdown"),
            other => panic!("attendu DangerousBlocked, obtenu {other:?}"),
        }

        // Ligne entierement inoffensive : risque agrege.
        assert_eq!(
            inspect_command("ls; git status").risk(),
            Some(RiskLevel::ReadOnly)
        );
        assert_eq!(
            inspect_command("ls; mkdir -p out").risk(),
            Some(RiskLevel::Mutating)
        );
    }

    #[test]
    fn test_robustesse_aucun_panic_entrees_hostiles() {
        let hostile = [
            "",
            " ",
            "\t\n\r",
            ";",
            ";;; ;;;",
            "|||",
            "&&&",
            "|",
            "&",
            "\u{0}\u{1}\u{7f}",
            "echo 'non ferme",
            "echo \"non ferme",
            "echo `non ferme",
            "echo \"abc\\",
            "\\",
            "$",
            "$(",
            "$()",
            "$((",
            ")))",
            "echo ${HOME",
            "echo \u{1f642} \u{1f680}; vim \u{1f642}",
            "éàü; ls",
            "echo $((1+2))",
            ":(){ :|:& };:",
            "a|b|c|d|e",
            "a && b || c ; d\n e",
            "sudo",
            "sudo -u",
            "timeout",
            "env",
            "bash -c",
            "powershell -Command",
        ];
        for cmd in hostile {
            let verdict = inspect_command(cmd);
            // Simple verification de non-panic + formatage du verdict.
            assert!(!format!("{verdict:?}").is_empty(), "entree {cmd:?}");
        }

        // Entrees tres longues.
        let very_long = format!("ls {}", "a".repeat(100_000));
        assert_eq!(inspect_command(&very_long).risk(), Some(RiskLevel::ReadOnly));

        let many_pipes = format!("ls {}", "| grep x ".repeat(400));
        assert!(inspect_command(&many_pipes).is_allowed());

        let many_segments = "ls; ".repeat(2_000);
        assert!(inspect_command(&many_segments).is_allowed());

        // Sous-commandes profondement imbriquees : la recursion est bornee.
        let deep = format!("echo {}ok{}", "$(".repeat(300), ")".repeat(300));
        assert!(inspect_command(&deep).is_allowed());

        // Sequence d'echappements et de quotes alternee.
        let tangle = "echo \\' \\\" ` `` ''' \"\"\" \\\\\\\\".repeat(50);
        let _ = inspect_command(&tangle);

        // Entree non fermee sur une redirection.
        assert_eq!(
            inspect_command("echo x > ").risk(),
            Some(RiskLevel::Mutating)
        );
    }
}
