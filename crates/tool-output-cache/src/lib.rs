//! Output cache for read-only tool calls.
//!
//! `tool-output-cache` memoizes the textual results of `read_file`,
//! `list_files` and `grep_search` during a single agent run. Entries are
//! evicted with an LRU policy and must be invalidated as soon as a write-like
//! operation touches the underlying path.
//!
//! The crate is dependency free (`std` only) and forbids `unsafe` code.
//!
//! ```
//! use tool_output_cache::ToolCache;
//!
//! let mut cache = ToolCache::new(4);
//! cache.insert("read_file", "src/main.rs", "fn main() {}", Some("src/main.rs"));
//! assert_eq!(cache.get("read_file", "src/main.rs"), Some("fn main() {}"));
//! cache.invalidate_path("src/main.rs");
//! assert_eq!(cache.get("read_file", "src/main.rs"), None);
//! ```

#![forbid(unsafe_code)]

pub mod cache;

pub use cache::{CacheStats, ToolCache};
