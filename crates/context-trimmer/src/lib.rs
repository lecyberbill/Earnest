//! `context-trimmer` : primitives de reduction de contexte pour agents
//! conversationnels.
//!
//! Expose les invariants detailles du module [`trimmer`].

pub mod trimmer;

pub use trimmer::{
    message_char_len, trim_history, truncate_tool_output, Message, Role, ToolCall,
    TRUNCATION_MARKER,
};