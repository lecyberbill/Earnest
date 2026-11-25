//! Brique partagée de résilience ReAct entre `earnest-core` et `earnest-ide-core` :
//! - Rolling Working Memory (`compact_with_rolling_memory`)
//! - Filet de sécurité par budget de tokens (`compact_by_tokens`)
//! - Détection d'annonces sans appel d'outil (`looks_like_unfinished_thought`)
//! - Filtrage d'outils MCP selon le prompt (`McpFilterDecision`)
//! - Détection de patterns répétitifs -> proposition de brouillon WFL (`.wf.yaml`)

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionMessage {
    pub role: String,
    pub content: String,
    pub has_images: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpFilterDecision {
    All,
    None,
    AllowServers(Vec<&'static str>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WflSuggestion {
    pub pattern_summary: String,
    pub repetitions: usize,
    pub yaml: String,
}

/// Décide quels serveurs MCP exposer selon l'intention du prompt lorsque le catalogue dépasse 8 outils.
pub fn decide_mcp_servers_for_prompt(prompt: &str, total_tools: usize) -> McpFilterDecision {
    if total_tools <= 8 {
        return McpFilterDecision::All;
    }
    let lower = prompt.to_lowercase();
    let broad = lower.contains("auto-éval")
        || lower.contains("auto éval")
        || lower.contains("selftest")
        || lower.contains("mcp")
        || lower.contains("capacit");
    if broad {
        return McpFilterDecision::All;
    }
    let want_wfl = lower.contains("wfl")
        || lower.contains("workflow")
        || lower.contains("yaml")
        || lower.contains("dry_run")
        || lower.contains("seal");
    let want_bcpe = lower.contains("bcpe")
        || lower.contains("kb")
        || lower.contains("connaissance")
        || lower.contains("knowledge")
        || lower.contains("incident")
        || lower.contains("produit");

    let mut servers = Vec::new();
    if want_wfl {
        servers.push("wfl");
    }
    if want_bcpe {
        servers.push("bcpe");
    }
    if servers.is_empty() {
        McpFilterDecision::None
    } else {
        McpFilterDecision::AllowServers(servers)
    }
}

/// Compacte l'historique si `total_chars > soft_limit` et `len > 18`, en insérant un bloc
/// `[ROLLING WORKING MEMORY ...]` qui conserve les points clés des messages évincés.
pub fn compact_with_rolling_memory(messages: &mut Vec<CompactionMessage>, soft_limit: usize) {
    let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();
    if total_chars <= soft_limit || messages.len() <= 18 {
        return;
    }
    let keep_head = 2usize.min(messages.len());
    let keep_tail = 16usize;
    if messages.len() <= keep_head + keep_tail {
        return;
    }
    let drop_end = messages.len() - keep_tail;
    let dropped_slice = &messages[keep_head..drop_end];

    let mut summary_lines: Vec<String> = Vec::new();
    for msg in dropped_slice {
        if msg.content.starts_with("[ROLLING WORKING MEMORY") {
            for line in msg.content.lines().skip(1) {
                if !line.trim().is_empty() {
                    summary_lines.push(line.to_string());
                }
            }
            continue;
        }
        let first_line = msg
            .content
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim();
        if first_line.is_empty() {
            continue;
        }
        let snippet: String = first_line.chars().take(150).collect();
        summary_lines.push(format!("- [{}] {}", msg.role, snippet));
    }
    if summary_lines.len() > 24 {
        let start_idx = summary_lines.len() - 24;
        summary_lines = summary_lines[start_idx..].to_vec();
    }

    let mut compacted = Vec::with_capacity(keep_head + keep_tail + 1);
    compacted.extend_from_slice(&messages[..keep_head]);
    compacted.push(CompactionMessage {
        role: "user".to_string(),
        content: format!(
            "[ROLLING WORKING MEMORY — {} anciens messages résumés, ne pas refaire les étapes déjà acquises]\n{}",
            dropped_slice.len(),
            summary_lines.join("\n")
        ),
        has_images: false,
    });
    compacted.extend_from_slice(&messages[drop_end..]);
    *messages = compacted;
}

/// Filet de sécurité par budget de tokens (`token-compactor`) si `messages` dépasse `max_chars`.
pub fn compact_by_tokens(messages: &mut Vec<CompactionMessage>, max_chars: usize) {
    if messages.iter().any(|m| m.has_images) {
        return;
    }
    let token_budget = (max_chars / token_compactor::TokenEstimator::CHARS_PER_TOKEN)
        .max(token_compactor::Compactor::MIN_BUDGET);
    let tc_msgs: Vec<token_compactor::Message> = messages
        .iter()
        .map(|m| {
            let role = match m.role.as_str() {
                "system" => token_compactor::Role::System,
                "assistant" => token_compactor::Role::Assistant,
                "tool" => token_compactor::Role::Tool,
                _ => token_compactor::Role::User,
            };
            token_compactor::Message::new(role, m.content.clone())
        })
        .collect();
    if token_compactor::TokenEstimator::estimate_messages(&tc_msgs) <= token_budget {
        return;
    }
    if let Ok(res) = token_compactor::Compactor::new(12).compact(tc_msgs, token_budget) {
        *messages = res
            .messages
            .into_iter()
            .map(|m| {
                let role = match m.role {
                    token_compactor::Role::System => "system",
                    token_compactor::Role::Assistant => "assistant",
                    token_compactor::Role::Tool => "tool",
                    token_compactor::Role::User => "user",
                };
                CompactionMessage {
                    role: role.to_string(),
                    content: m.content,
                    has_images: false,
                }
            })
            .collect();
    }
}

/// Détecte si une réponse texte sans `tool_calls` ressemble à une annonce intermédiaire.
pub fn looks_like_unfinished_thought(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.len() >= 500 {
        return false;
    }
    let lower = trimmed.to_lowercase();
    if trimmed.ends_with(':') {
        return true;
    }
    [
        "je vais ",
        "je relance ",
        "je scelle ",
        "je continue ",
        "je passe ",
        "je teste ",
        "je crée ",
        "je sauvegarde ",
        "je valide ",
        "je tente ",
        "je procède ",
        "je commence ",
        "je lance ",
        "je vérifie",
        "j'exécute ",
        "utilisons ",
        "exécutons ",
        "let me ",
        "let's ",
        "now let",
        "i will ",
        "i'll ",
        "next, i ",
        "je corrige",
        "j'ajoute",
        "je modifie",
        "séparateur powershell",
    ]
    .iter()
    .any(|pat| lower.contains(pat))
}

/// Analyse la trace d'outils d'un run pour détecter un pattern répétitif déterministe
/// (ex: boucle `[A, B, A, B]` ou appel répété ≥ 3 fois du même outil) et génère
/// un brouillon `.wf.yaml` à soumettre à validation humaine (jamais exécuté d'office).
pub fn suggest_wfl_from_tool_trace(
    run_id: &str,
    tool_calls: &[(String, String)],
) -> Option<WflSuggestion> {
    if tool_calls.len() < 4 {
        return None;
    }
    let names: Vec<&str> = tool_calls.iter().map(|(n, _)| n.as_str()).collect();

    // 1. Détection de motif cyclique de longueur 2..=4 répété consécutivement
    for pat_len in 2..=4usize {
        if names.len() < pat_len * 2 {
            continue;
        }
        for start in 0..=names.len() - pat_len * 2 {
            let pattern = &names[start..start + pat_len];
            let mut reps = 1usize;
            let mut cursor = start + pat_len;
            while cursor + pat_len <= names.len() && &names[cursor..cursor + pat_len] == pattern {
                reps += 1;
                cursor += pat_len;
            }
            if reps >= 2 {
                let steps_yaml = build_wfl_steps(&tool_calls[start..start + pat_len]);
                let summary = pattern.join(" -> ");
                return Some(WflSuggestion {
                    pattern_summary: summary.clone(),
                    repetitions: reps,
                    yaml: format_wfl_draft(run_id, &summary, reps, &steps_yaml),
                });
            }
        }
    }

    // 2. Détection d'un même outil appelé >= 4 fois dans le run
    let mut best_tool: Option<(&str, usize)> = None;
    for &n in &names {
        let cnt = names.iter().filter(|&&x| x == n).count();
        if cnt >= 4 && best_tool.map(|(_, c)| cnt > c).unwrap_or(true) {
            best_tool = Some((n, cnt));
        }
    }
    if let Some((tool_name, count)) = best_tool {
        let sample: Vec<(String, String)> = tool_calls
            .iter()
            .filter(|(n, _)| n == tool_name)
            .take(1)
            .cloned()
            .collect();
        let steps_yaml = build_wfl_steps(&sample);
        return Some(WflSuggestion {
            pattern_summary: format!("{} (x{})", tool_name, count),
            repetitions: count,
            yaml: format_wfl_draft(run_id, tool_name, count, &steps_yaml),
        });
    }

    None
}

fn build_wfl_steps(items: &[(String, String)]) -> String {
    let mut out = String::new();
    for (idx, (tool, args)) in items.iter().enumerate() {
        let safe_args = args.replace('\n', " ").replace('"', "\\\"");
        let snippet: String = safe_args.chars().take(180).collect();
        out.push_str(&format!(
            "  - id: step_{}\n    tool: \"{}\"\n    args_sample: \"{}\"\n",
            idx + 1,
            tool,
            snippet
        ));
    }
    out
}

fn format_wfl_draft(run_id: &str, summary: &str, reps: usize, steps_yaml: &str) -> String {
    format!(
        "# Brouillon WFL généré automatiquement depuis le run {run_id}\n\
         # Motif détecté: {summary} (x{reps}) — à relire et sceller avant activation\n\
         version: \"1\"\n\
         name: \"draft-{run_id}\"\n\
         enabled: false\n\
         steps:\n\
         {steps_yaml}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_memory_preserves_head_and_tail() {
        let mut msgs: Vec<CompactionMessage> = (0..25)
            .map(|i| CompactionMessage {
                role: if i == 0 { "system" } else { "user" }.into(),
                content: format!("Msg {} {}", i, "x".repeat(400)),
                has_images: false,
            })
            .collect();
        compact_with_rolling_memory(&mut msgs, 2000);
        assert_eq!(msgs.len(), 19);
        assert!(msgs[2].content.contains("ROLLING WORKING MEMORY"));
    }

    #[test]
    fn suggests_wfl_when_tool_sequence_repeats() {
        let trace = vec![
            ("read_file".into(), "{\"path\":\"a.rs\"}".into()),
            ("terminal_exec".into(), "{\"command\":\"cargo check\"}".into()),
            ("read_file".into(), "{\"path\":\"b.rs\"}".into()),
            ("terminal_exec".into(), "{\"command\":\"cargo check\"}".into()),
        ];
        let sug = suggest_wfl_from_tool_trace("run_test_1", &trace).expect("should suggest WFL");
        assert_eq!(sug.repetitions, 2);
        assert!(sug.pattern_summary.contains("read_file -> terminal_exec"));
        assert!(sug.yaml.contains("enabled: false"));
    }

    #[test]
    fn detects_english_and_french_unfinished_thoughts() {
        assert!(looks_like_unfinished_thought(
            "Let me verify the new validate_calendar body and build/test."
        ));
        assert!(looks_like_unfinished_thought(
            "Let's run cargo test and fix any remaining issues."
        ));
        assert!(looks_like_unfinished_thought(
            "Je corrige le dernier invariant avant de lancer cargo test."
        ));
        assert!(!looks_like_unfinished_thought(
            "Tous les contrats et tests unitaires sont au vert (30/30). Le README a été généré."
        ));
    }
}
