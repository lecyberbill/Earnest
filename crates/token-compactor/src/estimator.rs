use crate::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenEstimator;

impl TokenEstimator {
    pub const MESSAGE_OVERHEAD: usize = 4;
    pub const CHARS_PER_TOKEN: usize = 4;

    pub fn estimate_text(text: &str) -> usize {
        let chars = text.chars().count();
        if chars == 0 {
            0
        } else {
            let whole = chars / Self::CHARS_PER_TOKEN;
            let remainder = chars % Self::CHARS_PER_TOKEN;
            whole + if remainder == 0 { 0 } else { 1 }
        }
    }

    pub fn estimate_message(message: &Message) -> usize {
        Self::MESSAGE_OVERHEAD + Self::estimate_text(&message.content)
    }

    pub fn estimate_messages(messages: &[Message]) -> usize {
        messages.iter().fold(0usize, |acc, m| acc + Self::estimate_message(m))
    }
}

impl Default for TokenEstimator {
    fn default() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Role;

    #[test]
    fn estimator_deterministic_and_fixed_overhead() {
        let messages = vec![
            Message {
                role: Role::System,
                content: String::new(),
            },
            Message {
                role: Role::User,
                content: "hello world".to_string(),
            },
        ];

        let first = TokenEstimator::estimate_messages(&messages);
        let second = TokenEstimator::estimate_messages(&messages);
        assert_eq!(first, second);

        let expected =
            TokenEstimator::MESSAGE_OVERHEAD + TokenEstimator::estimate_text("")
                + TokenEstimator::MESSAGE_OVERHEAD + TokenEstimator::estimate_text("hello world");
        assert_eq!(first, expected);

        let only_overhead = vec![
            Message {
                role: Role::System,
                content: String::new(),
            },
            Message {
                role: Role::Assistant,
                content: String::new(),
            },
            Message {
                role: Role::Tool,
                content: String::new(),
            },
        ];
        assert_eq!(
            TokenEstimator::estimate_messages(&only_overhead),
            3 * TokenEstimator::MESSAGE_OVERHEAD
        );

        assert_eq!(TokenEstimator::estimate_messages(&[]), 0);
    }
}
