#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Outside,
    InThought,
}

const OPEN_TAGS: &[&str] = &["<thought>", "\u{3C}think\u{3E}"];
const CLOSE_TAGS: &[&str] = &["</thought>", "\u{3C}/think\u{3E}"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub visible: String,
    pub thought: String,
}

pub struct StreamCleaner {
    state: State,
    pending: String,
}

impl StreamCleaner {
    pub fn new() -> Self {
        Self {
            state: State::Outside,
            pending: String::new(),
        }
    }

    pub fn push(&mut self, chunk: &str) -> Output {
        self.pending.push_str(chunk);
        let mut visible = String::new();
        let mut thought = String::new();

        loop {
            match self.state {
                State::Outside => {
                    if let Some((pos, tag_len)) = find_earliest(&self.pending, OPEN_TAGS) {
                        visible.push_str(&self.pending[..pos]);
                        self.pending = self.pending[pos + tag_len..].to_string();
                        self.state = State::InThought;
                    } else {
                        let keep = max_prefix_suffix(&self.pending, OPEN_TAGS);
                        let emit_len = self.pending.len() - keep;
                        visible.push_str(&self.pending[..emit_len]);
                        self.pending = self.pending[emit_len..].to_string();
                        break;
                    }
                }
                State::InThought => {
                    if let Some((pos, tag_len)) = find_earliest(&self.pending, CLOSE_TAGS) {
                        thought.push_str(&self.pending[..pos]);
                        self.pending = self.pending[pos + tag_len..].to_string();
                        self.state = State::Outside;
                    } else {
                        let keep = max_prefix_suffix(&self.pending, CLOSE_TAGS);
                        let emit_len = self.pending.len() - keep;
                        thought.push_str(&self.pending[..emit_len]);
                        self.pending = self.pending[emit_len..].to_string();
                        break;
                    }
                }
            }
        }

        Output { visible, thought }
    }

    pub fn finish(self) -> Output {
        let (visible, thought) = match self.state {
            State::Outside => (self.pending, String::new()),
            State::InThought => (String::new(), self.pending),
        };
        Output { visible, thought }
    }
}

impl Default for StreamCleaner {
    fn default() -> Self {
        Self::new()
    }
}

fn find_earliest(text: &str, tags: &[&str]) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for tag in tags {
        if let Some(pos) = text.find(tag) {
            match best {
                Some((bpos, _)) if bpos <= pos => {}
                _ => best = Some((pos, tag.len())),
            }
        }
    }
    best
}

fn max_prefix_suffix(text: &str, tags: &[&str]) -> usize {
    let mut max_keep = 0;
    for tag in tags {
        let max_len = std::cmp::min(tag.len(), text.len());
        for len in (1..=max_len).rev() {
            if text.ends_with(&tag[..len]) {
                if len > max_keep {
                    max_keep = len;
                }
                break;
            }
        }
    }
    max_keep
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complete_tag() {
        let mut sc = StreamCleaner::new();
        let out = sc.push("Hello <thought>internal reasoning</thought> World");
        assert_eq!(out.visible, "Hello  World");
        assert_eq!(out.thought, "internal reasoning");
    }

    #[test]
    fn test_split_tag() {
        let mut sc = StreamCleaner::new();
        let out1 = sc.push("Hello <th");
        assert_eq!(out1.visible, "Hello ");
        assert_eq!(out1.thought, "");
        let out2 = sc.push("ought>reason</thought>");
        assert_eq!(out2.visible, "");
        assert_eq!(out2.thought, "reason");
    }

    #[test]
    fn test_text_before_after() {
        let mut sc = StreamCleaner::new();
        let out = sc.push("Start text \u{3C}think\u{3E}Inner thought\u{3C}/think\u{3E}End text");
        assert_eq!(out.visible, "Start text End text");
        assert_eq!(out.thought, "Inner thought");
    }

    #[test]
    fn test_unclosed_tag_finish() {
        let mut sc = StreamCleaner::new();
        let out1 = sc.push("Text \u{3C}think\u{3E}");
        assert_eq!(out1.visible, "Text ");
        assert_eq!(out1.thought, "");
        let out2 = sc.push("reasoning without close");
        assert_eq!(out2.thought, "reasoning without close");
        let out3 = sc.finish();
        assert_eq!(out3.visible, "");
        assert_eq!(out3.thought, "");
    }

    #[test]
    fn test_no_tag() {
        let mut sc = StreamCleaner::new();
        let out = sc.push("Just plain text, no tags here.");
        assert_eq!(out.visible, "Just plain text, no tags here.");
        assert_eq!(out.thought, "");
    }

    #[test]
    fn test_think_tag() {
        let mut sc = StreamCleaner::new();
        let out = sc.push("Answer: \u{3C}think\u{3E}Step by step...\u{3C}/think\u{3E}Result: 42");
        assert_eq!(out.visible, "Answer: Result: 42");
        assert_eq!(out.thought, "Step by step...");
    }

    #[test]
    fn test_split_think_tag() {
        let mut sc = StreamCleaner::new();
        let out1 = sc.push("Hi \u{3C}thi");
        assert_eq!(out1.visible, "Hi ");
        assert_eq!(out1.thought, "");
        let out2 = sc.push("nk\u{3E}secret\u{3C}/th");
        assert_eq!(out2.visible, "");
        assert_eq!(out2.thought, "secret");
        let out3 = sc.push("ink\u{3E}!");
        assert_eq!(out3.visible, "!");
        assert_eq!(out3.thought, "");
    }
}
