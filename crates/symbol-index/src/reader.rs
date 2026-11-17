//! Targeted line-range reader to minimize token consumption.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A slice of a source file with 1-based line bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineSnippet {
    pub file: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    pub total_lines: usize,
    pub content: String,
}

/// Reads lines `[start_line..=end_line]` (1-based, inclusive) from `path`,
/// formatted with line numbers (`L12: ...`).
pub fn read_line_range(path: &Path, start_line: usize, end_line: usize) -> io::Result<LineSnippet> {
    let raw = fs::read_to_string(path)?;
    let lines: Vec<&str> = raw.lines().collect();
    let total_lines = lines.len();

    if total_lines == 0 {
        return Ok(LineSnippet {
            file: path.to_path_buf(),
            start_line: 0,
            end_line: 0,
            total_lines: 0,
            content: String::new(),
        });
    }

    let start = start_line.max(1).min(total_lines);
    let end = end_line.max(start).min(total_lines);

    let mut formatted = String::new();
    for (idx, line) in lines.iter().enumerate().take(end).skip(start - 1) {
        formatted.push_str(&format!("L{}: {}\n", idx + 1, line));
    }

    Ok(LineSnippet {
        file: path.to_path_buf(),
        start_line: start,
        end_line: end,
        total_lines,
        content: formatted,
    })
}

/// Reads a context window (`before` lines before, `after` lines after) around `center_line`.
pub fn read_symbol_snippet(
    path: &Path,
    center_line: usize,
    before: usize,
    after: usize,
) -> io::Result<LineSnippet> {
    let start = center_line.saturating_sub(before).max(1);
    let end = center_line.saturating_add(after);
    read_line_range(path, start, end)
}
