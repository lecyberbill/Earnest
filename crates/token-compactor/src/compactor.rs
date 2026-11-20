use crate::estimator::TokenEstimator;

/// Rôle d'un message dans la conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// Message de conversation compactable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}

/// Erreurs de compaction déterministes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactionError {
    /// Le budget est nul : aucune compaction possible.
    ZeroBudget,
    /// Le budget est inférieur au coût minimal incompressible
    /// (système initial + dernier prompt user, tous deux vidés).
    BudgetBelowMinimum { min_budget: usize },
}

/// Statistiques de la compaction (cohérence vérifiable par les sondes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionStats {
    pub original_tokens: usize,
    pub compacted_tokens: usize,
    pub messages_removed: usize,
    pub messages_truncated: usize,
    pub tool_responses_summarized: usize,
}

/// Résultat d'une compaction : messages retenus + statistiques.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionResult {
    pub messages: Vec<Message>,
    pub stats: CompactionStats,
}

/// Compactor déterministe par invariants :
/// 1) résumé des réponses d'outil avant suppression,
/// 2) éviction des tours intermédiaires les plus anciens,
/// 3) troncature bornée garantissant le respect strict du budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Compactor {
    keep_recent: usize,
}

impl Compactor {
    /// Coût minimal incompressible : deux messages protégés vidés
    /// (2 * MESSAGE_OVERHEAD + 2 * estimate_text("") = 2 * MESSAGE_OVERHEAD).
    pub const MIN_BUDGET: usize = 2 * TokenEstimator::MESSAGE_OVERHEAD;

    const SUMMARY_PREFIX: &'static str = "[summarized tool response: ";

    pub fn new(keep_recent: usize) -> Self {
        Self { keep_recent }
    }

    /// Indices protégés : message système initial, dernier prompt user,
    /// et fenêtre récente des `keep_recent` derniers messages.
    fn protected_indexes(messages: &[Message], keep_recent: usize) -> Vec<bool> {
        let n = messages.len();
        let mut p = vec![false; n];
        if n > 0 && messages[0].role == Role::System {
            p[0] = true;
        }
        if let Some(lu) = messages.iter().rposition(|m| m.role == Role::User) {
            p[lu] = true;
        }
        let start = n.saturating_sub(keep_recent);
        for slot in p.iter_mut().skip(start) {
            *slot = true;
        }
        p
    }

    /// Compacte `input` sous `budget` tokens (estimateur déterministe).
    pub fn compact(
        &self,
        input: Vec<Message>,
        budget: usize,
    ) -> Result<CompactionResult, CompactionError> {
        if budget == 0 {
            return Err(CompactionError::ZeroBudget);
        }
        if budget < Self::MIN_BUDGET {
            return Err(CompactionError::BudgetBelowMinimum {
                min_budget: Self::MIN_BUDGET,
            });
        }

        let original_tokens = TokenEstimator::estimate_messages(&input);
        let mut messages = input;
        let mut removed = 0usize;
        let mut truncated = 0usize;
        let mut summarized = 0usize;

        if TokenEstimator::estimate_messages(&messages) <= budget {
            return Ok(CompactionResult {
                messages,
                stats: CompactionStats {
                    original_tokens,
                    compacted_tokens: original_tokens,
                    messages_removed: 0,
                    messages_truncated: 0,
                    tool_responses_summarized: 0,
                },
            });
        }

        // Phase A : résumer les réponses d'outil AVANT de les supprimer.
        {
            let prot = Self::protected_indexes(&messages, self.keep_recent);
            for i in 0..messages.len() {
                if prot[i] {
                    continue;
                }
                let m = &mut messages[i];
                if m.role == Role::Tool && !m.content.is_empty() {
                    let chars = m.content.chars().count();
                    m.content = format!("{}{} chars]", Self::SUMMARY_PREFIX, chars);
                    summarized += 1;
                }
            }
        }

        // Phase B : éviction des tours intermédiaires les plus anciens
        // (on évite de tronquer les messages protégés tant qu'on peut
        // simplement supprimer un message non protégé).
        loop {
            if TokenEstimator::estimate_messages(&messages) <= budget {
                break;
            }
            let prot = Self::protected_indexes(&messages, self.keep_recent);
            match (0..messages.len()).find(|&i| !prot[i]) {
                Some(i) => {
                    messages.remove(i);
                    removed += 1;
                }
                None => break,
            }
        }

        // Phase C : troncature bornée (seulement si l'éviction ne suffit
        // pas à tenir sous le budget).
        if TokenEstimator::estimate_messages(&messages) > budget {
            truncated += Self::truncate_pass(&mut messages, budget);
        }

        // Cas dégénéré : on garantit TOUJOURS le budget (invariant C8).
        while TokenEstimator::estimate_messages(&messages) > budget && !messages.is_empty() {
            messages.remove(0);
            removed += 1;
        }

        let compacted_tokens = TokenEstimator::estimate_messages(&messages);
        Ok(CompactionResult {
            messages,
            stats: CompactionStats {
                original_tokens,
                compacted_tokens,
                messages_removed: removed,
                messages_truncated: truncated,
                tool_responses_summarized: summarized,
            },
        })
    }

    /// Tronque les contenus pour tenir sous `budget` en gardant tous
    /// les messages (troncuration bornée).
    fn truncate_pass(messages: &mut [Message], budget: usize) -> usize {
        let mut truncated = 0usize;
        for i in 0..messages.len() {
            let others: usize = messages
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, m)| TokenEstimator::estimate_message(m))
                .sum();
            let allowed = budget.saturating_sub(others);
            if TokenEstimator::estimate_message(&messages[i]) <= allowed {
                continue;
            }
            let current_chars = messages[i].content.chars().count();
            if allowed >= 1 {
                // Taille cible : `allowed` tokens (overhead inclus).
                let content_budget = allowed.saturating_sub(TokenEstimator::MESSAGE_OVERHEAD);
                let target_chars = if content_budget == 0 {
                    0
                } else {
                    (content_budget - 1) * TokenEstimator::CHARS_PER_TOKEN + 1
                };
                let new_len = messages[i].content.chars().take(target_chars).count();
                if new_len < current_chars {
                    messages[i].content = messages[i]
                        .content
                        .chars()
                        .take(target_chars)
                        .collect();
                    truncated += 1;
                }
            } else if current_chars > 0 {
                messages[i].content.clear();
                truncated += 1;
            }
        }
        truncated
    }
}

impl Default for Compactor {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convo() -> Vec<Message> {
        vec![
            Message::new(Role::System, "You are a helpful assistant."),
            Message::new(Role::User, "Old question one"),
            Message::new(Role::Assistant, "Old answer one"),
            Message::new(
                Role::Tool,
                "TOOL OUTPUT WITH A VERY LONG PAYLOAD THAT EATS TOKENS QUICKLY IN THE CONTEXT WINDOW",
            ),
            Message::new(Role::User, "Old question two"),
            Message::new(Role::Assistant, "Old answer two"),
            Message::new(Role::User, "Latest question"),
        ]
    }

    #[test]
    fn rejects_zero_and_below_min_budget() {
        let c = Compactor::new(1);
        assert_eq!(c.compact(convo(), 0), Err(CompactionError::ZeroBudget));
        assert_eq!(
            c.compact(convo(), Compactor::MIN_BUDGET - 1),
            Err(CompactionError::BudgetBelowMinimum {
                min_budget: Compactor::MIN_BUDGET
            })
        );
    }

    #[test]
    fn preserves_initial_system_and_last_user_prompt() {
        let c = Compactor::new(1);
        let input = convo();
        let out = c.compact(input.clone(), 30).unwrap();
        assert_eq!(out.messages[0], input[0]);
        assert_eq!(*out.messages.last().unwrap(), *input.last().unwrap());
    }

    #[test]
    fn keep_recent_window_intact() {
        let c = Compactor::new(3);
        let input = convo();
        let out = c.compact(input.clone(), 30).unwrap();
        // 3 messages non protégés évacués → 4 conservés
        assert_eq!(out.messages.len(), input.len() - 3);
        // La fenêtre récente (3 derniers) doit être byte-identique à l'input
        let window: Vec<&Message> = input.iter().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect();
        let tail: Vec<&Message> = out.messages.iter().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect();
        assert_eq!(tail, window);
    }

    #[test]
    fn summarizes_tool_responses_before_removing() {
        let c = Compactor::new(1);
        let out = c.compact(convo(), 50).unwrap();
        assert_eq!(out.stats.tool_responses_summarized, 1);
        let tool = out.messages.iter().find(|m| m.role == Role::Tool).expect("tool kept as summary");
        assert!(tool.content.starts_with(Compactor::SUMMARY_PREFIX));
        assert!(!out.messages.iter().any(|m| m.content.contains("EATS TOKENS")));
    }

    #[test]
    fn evicts_oldest_intermediate_turns() {
        let c = Compactor::new(1);
        let out = c.compact(convo(), 30).unwrap();
        let contents: Vec<&str> = out.messages.iter().map(|m| m.content.as_str()).collect();
        assert!(!contents.contains(&"Old question one"));
        assert!(!contents.contains(&"Old answer one"));
        assert!(contents.contains(&"Old answer two"));
        assert_eq!(out.messages.last().unwrap().content, "Latest question");
        assert!(out.stats.messages_removed >= 3);
    }

    #[test]
    fn bounded_truncation_respects_budget() {
        let c = Compactor::new(0);
        let input = vec![
            Message::new(Role::System, "SYS ".repeat(40)),
            Message::new(Role::User, "USR ".repeat(40)),
        ];
        let original = TokenEstimator::estimate_messages(&input);
        let out = c.compact(input.clone(), 30).unwrap();
        let result_tokens = TokenEstimator::estimate_messages(&out.messages);
        assert!(result_tokens <= 30);
        assert!(result_tokens < original);
        assert_eq!(out.messages.len(), 2, "troncuration bornée, pas suppression");
        assert!(out.messages[0].content.chars().count() < original_content_chars(&input[0]));
        assert_eq!(out.stats.messages_truncated, 2);
        assert_eq!(out.stats.messages_removed, 0);
    }

    fn original_content_chars(m: &Message) -> usize {
        m.content.chars().count()
    }

    #[test]
    fn result_always_within_budget() {
        for budget in [10usize, 20, 30, 40, 50, 60, 88] {
            let c = Compactor::new(1);
            let out = c.compact(convo(), budget).unwrap();
            let total = TokenEstimator::estimate_messages(&out.messages);
            assert!(total <= budget, "budget={budget} total={total}");
            assert!(budget >= Compactor::MIN_BUDGET);
        }
    }

    #[test]
    fn compaction_stats_consistency() {
        let c = Compactor::new(1);
        let input = convo();
        let out = c.compact(input.clone(), 50).unwrap();
        assert_eq!(out.stats.original_tokens, TokenEstimator::estimate_messages(&input));
        assert_eq!(out.stats.compacted_tokens, TokenEstimator::estimate_messages(&out.messages));
        assert_eq!(
            out.messages.len() + out.stats.messages_removed,
            input.len()
        );
        assert!(out.stats.compacted_tokens <= 50);
        assert!(out.stats.original_tokens > out.stats.compacted_tokens);
    }

    #[test]
    fn compaction_deterministic() {
        let c = Compactor::new(2);
        let a = c.compact(convo(), 40).unwrap();
        let b = c.compact(convo(), 40).unwrap();
        assert_eq!(a, b);
    }
}