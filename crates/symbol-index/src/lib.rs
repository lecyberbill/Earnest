//! # symbol-index
//!
//! Pure-Rust workspace symbol indexer (`fn`, `struct`, `enum`, `trait`, `impl`,
//! `class`, `def`, `function`, `interface`, `type`, `export`) with fuzzy search
//! and targeted line-range reading. Zero external dependencies (`std` only).

pub mod indexer;
pub mod reader;

pub use indexer::{SymbolEntry, SymbolIndex, SymbolKind};
pub use reader::{LineSnippet, read_line_range, read_symbol_snippet};
