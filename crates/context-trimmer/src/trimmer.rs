//! Coeur metier de `context-trimmer` : troncature de sorties d'outils et
//! fenetrage d'historique de conversation.
//!
//! ## Invariants (contrats scelles)
//!
//! - **C1** `truncate_tool_output` conserve la tete *et* la queue quand le
//!   contenu depasse la limite, et renvoie le contenu strictement inchange
//!   sinon. Le marqueur [`TRUNCATION_MARKER`] est deduit du budget avant
//!   partage tete/queue (la tete est privilegiee d'un caractere si le reste
//!   est impair).
//! - **C2** `trim_history` epingle inconditionnellement le premier message
//!   `Role::System`, meme si son cout depasse le budget.
//! - **C3** `trim_history` respecte le budget exprime en caracteres Unicode
//!   (`char`), sauf pour le message systeme epingle (C2 prime).
//! - **C4** `trim_history` ne separe jamais un appel d'outil de son/ses
//!   resultats : le fenetrage se fait a granularite d'unite.
//!
//! ## Mesure
//!
//! Le cout d'un message est `content.chars().count()`. Les metadonnees
//! (`tool_calls`, `tool_call_id`) ne sont pas comptees : simplification
//! assumee. Tout est exprime en `char` pour ne jamais couper un caractere
//! multi-octets.

/// Marqueur insere a la place du contenu retire par [`truncate_tool_output`].
///
/// Longueur : 28 caracteres (utilisee pour deduire le budget restant).
pub const TRUNCATION_MARKER: &str = "\n--- [output truncated] ---\n";

// ---------------------------------------------------------------- Types

/// Role d'un message dans l'historique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Consigne systeme (epinglee par C2).
    System,
    /// Intervient cote utilisateur / appelant.
    User,
    /// Reponse du modele.
    Assistant,
    /// Resultat d'un outil, rattache a un appel par `tool_call_id`.
    Tool,
}

/// Appel d'outil emis par l'assistant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    /// Identifiant unique de l'appel (relie le resultat a l'appel).
    pub id: String,
    /// Nom de l'outil invoque.
    pub name: String,
    /// Arguments serialises (JSON la plupart du temps).
    pub arguments: String,
}

impl ToolCall {
    /// Construit un appel d'outil.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        }
    }
}

/// Message d'une conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Role emetteur.
    pub role: Role,
    /// Contenu textuel (seul champ mesure par le budget).
    pub content: String,
    /// Appels d'outils emis (assistant uniquement).
    pub tool_calls: Vec<ToolCall>,
    /// Identifiant de l'appel auquel ce message repond (`Role::Tool`).
    pub tool_call_id: Option<String>,
}

impl Message {
    fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    /// Message systeme.
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(Role::System, content)
    }

    /// Message utilisateur.
    pub fn user(content: impl Into<String>) -> Self {
        Self::new(Role::User, content)
    }

    /// Message assistant sans appel d'outil.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(Role::Assistant, content)
    }

    /// Message assistant porteur d'appels d'outils.
    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<ToolCall>,
    ) -> Self {
        let mut message = Self::new(Role::Assistant, content);
        message.tool_calls = tool_calls;
        message
    }

    /// Resultat d'outil rattache a l'appel `tool_call_id`.
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        let mut message = Self::new(Role::Tool, content);
        message.tool_call_id = Some(tool_call_id.into());
        message
    }

    /// Vrai si ce message emet au moins un appel d'outil.
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
}

/// Cout d'un message en caracteres Unicode.
pub fn message_char_len(message: &Message) -> usize {
    message.content.chars().count()
}

// ---------------------------------------------------------------- Troncature (C1)

/// Borne une sortie d'outil volumineuse a `max_chars` caracteres.
///
/// - Si `content` tient dans `max_chars` : renvoye **strictement inchange**.
/// - Sinon : conserve la tete et la queue, separees par
///   [`TRUNCATION_MARKER`]. La queue porte l'information utile (code de
///   sortie, derniere ligne de log), d'ou le partage symetrique.
pub fn truncate_tool_output(content: &str, max_chars: usize) -> String {
    let total = content.chars().count();
    if total <= max_chars {
        return content.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }

    let marker_len = TRUNCATION_MARKER.chars().count();
    if max_chars <= marker_len {
        // Budget trop court pour tete + queue : on ne peut livrer que le
        // marqueur, tronque a la limite demandee.
        return TRUNCATION_MARKER.chars().take(max_chars).collect();
    }

    let room = max_chars - marker_len;
    let head_len = room - room / 2; // tete privilegiee si impair
    let tail_len = room / 2;

    let mut out = String::new();
    out.extend(content.chars().take(head_len));
    out.push_str(TRUNCATION_MARKER);
    if tail_len > 0 {
        out.extend(content.chars().skip(total - tail_len));
    }
    out
}

// ---------------------------------------------------------------- Fenetrage (C2..C4)

/// Decoupe l'historique en unites indivisibles.
///
/// Une unite est soit un message seul, soit un message assistant porteur
/// d'appels suivi immediatement de ses resultats d'outil (`Role::Tool`). Une
/// unite est conservee ou supprimee d'un bloc : c'est ce qui garantit C4.
fn units_of(messages: &[Message]) -> Vec<(usize, usize)> {
    let mut units = Vec::new();
    let mut start = 0;
    while start < messages.len() {
        let mut end = start + 1;
        if messages[start].has_tool_calls() {
            while end < messages.len() && messages[end].role == Role::Tool {
                end += 1;
            }
        }
        units.push((start, end));
        start = end;
    }
    units
}

/// Cout en caracteres d'une tranche de messages.
fn range_char_len(messages: &[Message]) -> usize {
    messages.iter().map(message_char_len).sum()
}

/// Fenetre l'historique pour tenir dans `budget` caracteres.
///
/// Le premier message systeme est epingle inconditionnellement (C2), y compris
/// si son cout depasse `budget`. Le reste est une fenetre **contigue** des
/// unites les plus recentes : on parcourt de la queue vers la tete et on
/// s'arrete a la premiere unite qui ne rentre pas (pas de trou).
pub fn trim_history(history: &[Message], budget: usize) -> Vec<Message> {
    if history.is_empty() {
        return Vec::new();
    }

    let units = units_of(history);
    let pins_system = history[0].role == Role::System;

    let (first_free_unit, mut remaining) = if pins_system {
        (
            1usize,
            budget.saturating_sub(message_char_len(&history[0])),
        )
    } else {
        (0usize, budget)
    };

    let mut kept: Vec<(usize, usize)> = Vec::new();
    let mut cursor = units.len();
    while cursor > first_free_unit {
        cursor -= 1;
        let (start, end) = units[cursor];
        let cost = range_char_len(&history[start..end]);
        if cost <= remaining {
            remaining -= cost;
            kept.push((start, end));
        } else {
            break; // fenetre contigue : on ne saute pas par-dessus
        }
    }
    kept.reverse();

    let mut out: Vec<Message> = Vec::new();
    if pins_system {
        out.push(history[0].clone());
    }
    for (start, end) in kept {
        out.extend_from_slice(&history[start..end]);
    }
    out
}
