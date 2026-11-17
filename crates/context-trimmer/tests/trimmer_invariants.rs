//! Sondes d'invariants C1..C4.
//!
//! Les tests vivent a la racine d'un fichier d'integration afin que leur nom
//! complet soit exactement le nom de la fonction (les sondes utilisent
//! `cargo test <nom> -- --exact`).

use context_trimmer::{trim_history, truncate_tool_output, Message, Role, ToolCall};

fn total_content_chars(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| m.content.chars().count())
        .sum()
}

// ---------------------------------------------------------------- C1

#[test]
fn truncate_tool_output_keeps_head_and_tail_when_over_limit() {
    let content = format!("HEAD:{}{}", "x".repeat(400), ":TAIL");
    let max_chars = 100;

    let trimmed = truncate_tool_output(&content, max_chars);

    assert!(
        trimmed.chars().count() <= max_chars,
        "budget depasse: {} > {}",
        trimmed.chars().count(),
        max_chars
    );
    assert!(trimmed.starts_with("HEAD:"), "tete perdue: {trimmed}");
    assert!(trimmed.ends_with(":TAIL"), "queue perdue: {trimmed}");
    assert!(trimmed.contains("[output truncated]"), "marqueur absent: {trimmed}");
    assert!(trimmed.chars().count() < content.chars().count());

    // Robustesse UTF-8 : jamais de coupure au milieu d'un caractere.
    let accented = format!("DEBUT{}{}", "e\u{301}".repeat(300), "FIN");
    let trimmed_accented = truncate_tool_output(&accented, 60);
    assert!(trimmed_accented.chars().count() <= 60);
    assert!(trimmed_accented.starts_with("DEBUT"));
    assert!(trimmed_accented.ends_with("FIN"));
    assert!(trimmed_accented.contains("[output truncated]"));
}

#[test]
fn truncate_tool_output_returns_unchanged_when_under_limit() {
    let content = "short tool output";
    let len = content.chars().count();

    assert_eq!(truncate_tool_output(content, len), content);
    assert_eq!(truncate_tool_output(content, 10_000), content);
    assert_eq!(truncate_tool_output("", 0), "");

    let exact = "a".repeat(50);
    assert_eq!(truncate_tool_output(&exact, 50), exact);
}

// ---------------------------------------------------------------- C2

#[test]
fn trim_history_preserves_first_system_message() {
    let system = Message::system("SYSTEM PROMPT: vous etes un assistant rigoureux.");
    let mut history = vec![system.clone()];
    for i in 0..10 {
        history.push(Message::user(format!("question {i}")));
    }

    // Budget trop court pour autre chose que le message epingle.
    let tiny = trim_history(&history, 5);
    assert_eq!(tiny.len(), 1, "seul le systeme devait rester: {tiny:?}");
    assert_eq!(tiny[0].role, Role::System);
    assert_eq!(tiny[0], system);

    // Budget nul : le systeme reste epingle (invariant critique).
    let zero = trim_history(&history, 0);
    assert_eq!(zero.len(), 1, "le systeme doit survivre a un budget nul: {zero:?}");
    assert_eq!(zero[0], system);

    // Budget confortable : le systeme est toujours en tete, contenu intact.
    let roomy = trim_history(&history, 200);
    assert!(roomy.len() > 1);
    assert_eq!(roomy[0].role, Role::System);
    assert_eq!(roomy[0].content, system.content);
}

// ---------------------------------------------------------------- C3

#[test]
fn trim_history_enforces_char_budget() {
    let history = vec![
        Message::system("sys"),
        Message::user("a".repeat(20)),
        Message::user("b".repeat(20)),
        Message::user("c".repeat(20)),
    ];

    let budget = 43; // 3 (systeme) + 20 + 20
    let trimmed = trim_history(&history, budget);
    assert!(
        total_content_chars(&trimmed) <= budget,
        "budget viole: {}",
        total_content_chars(&trimmed)
    );
    assert_eq!(trimmed[0].role, Role::System);
    assert_eq!(trimmed.len(), 3, "fenetre recente attendue: {trimmed:?}");
    assert_eq!(trimmed[1].content, "b".repeat(20));
    assert_eq!(trimmed.last().unwrap().content, "c".repeat(20));

    // Budget large : tout est conserve, dans l'ordre.
    let all = trim_history(&history, 1_000);
    assert_eq!(all, history);

    // Budget nul : seul le systeme epingle subsiste.
    let none = trim_history(&history, 0);
    assert_eq!(none.len(), 1);

    // Historique vide : resultat vide.
    assert!(trim_history(&[], 100).is_empty());
}

// ---------------------------------------------------------------- C4

#[test]
fn trim_history_never_splits_tool_call_result_pair() {
    let history = vec![
        Message::system("S"),
        Message::user("U".repeat(10)),
        Message::assistant_with_tool_calls(
            "A".repeat(10),
            vec![ToolCall::new("call_1", "read_file", "{}")],
        ),
        Message::tool_result("call_1", "R".repeat(20)),
        Message::user("X".repeat(30)),
        Message::assistant("Y".repeat(30)),
    ];

    // Budget calibre : une implementation gloutonne message par message
    // conserverait le resultat d'outil (20 car.) sans son appel (10 car.).
    // Ici la paire coute 30 et doit etre ecartee d'un bloc.
    let budget = 86;
    let trimmed = trim_history(&history, budget);
    let has_call = trimmed.iter().any(|m| m.has_tool_calls());
    let has_result = trimmed.iter().any(|m| m.role == Role::Tool);

    assert_eq!(
        has_call, has_result,
        "paire appel/resultat dissociee: {trimmed:?}"
    );
    assert!(!has_result, "resultat d'outil orphelin conserve: {trimmed:?}");
    assert!(total_content_chars(&trimmed) <= budget);

    // Budget suffisant : la paire est conservee integralement.
    let roomy_budget = 91;
    let kept = trim_history(&history, roomy_budget);
    let call_message = kept
        .iter()
        .find(|m| m.has_tool_calls())
        .expect("appel d'outil attendu dans la fenetre conservee");
    let result_message = kept
        .iter()
        .find(|m| m.role == Role::Tool)
        .expect("resultat d'outil attendu dans la fenetre conservee");
    assert!(call_message.tool_calls.iter().any(|c| c.id == "call_1"));
    assert_eq!(result_message.tool_call_id.as_deref(), Some("call_1"));
    assert!(total_content_chars(&kept) <= roomy_budget);

    // Un resultat d'outil sans appel correspondant (entree deja inconsistante)
    // ne doit jamais faire paniquer le fenetrage.
    let orphan = vec![Message::system("S"), Message::tool_result("ghost", "Z".repeat(10))];
    let handled = trim_history(&orphan, 1_000);
    assert_eq!(handled.len(), 2);
}