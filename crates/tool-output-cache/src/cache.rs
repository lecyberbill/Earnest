//! In-memory cache for read-only tool outputs.
//!
//! The cache stores the textual result of read-only operations
//! (`read_file`, `list_files`, `grep_search`) so that an agent run does not
//! repeat identical calls. Any write-like operation must call
//! [`ToolCache::invalidate_path`] (or [`ToolCache::clear`]) to drop stale data.

use std::collections::{HashMap, HashSet};

/// Separator used to build the composite internal key from `(tool, args_key)`.
const SEP: char = '\u{1F}';

/// Cumulative telemetry snapshot of a [`ToolCache`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    /// Number of successful lookups since construction.
    pub hits: u64,
    /// Number of failed lookups since construction.
    pub misses: u64,
    /// Number of entries currently alive in the cache.
    pub len: usize,
}

#[derive(Debug, Clone)]
struct Entry {
    output: String,
    path_hint: Option<String>,
    seq: u64,
}

/// LRU cache keyed by `(tool, args_key)` with path based invalidation.
#[derive(Debug, Clone)]
pub struct ToolCache {
    max_entries: usize,
    entries: HashMap<String, Entry>,
    path_index: HashMap<String, HashSet<String>>,
    hits: u64,
    misses: u64,
    clock: u64,
}

impl ToolCache {
    /// Creates a cache holding at most `max_entries` entries.
    ///
    /// `max_entries == 0` disables the cache: [`insert`](Self::insert) becomes a
    /// no-op and every [`get`](Self::get) is a miss.
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries,
            entries: HashMap::new(),
            path_index: HashMap::new(),
            hits: 0,
            misses: 0,
            clock: 0,
        }
    }

    /// Configured capacity.
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// Number of live entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` when no entry is cached.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns `true` when `(tool, args_key)` is currently cached.
    pub fn contains(&self, tool: &str, args_key: &str) -> bool {
        self.entries.contains_key(&Self::make_key(tool, args_key))
    }

    /// Looks up a cached output.
    ///
    /// A hit refreshes the recency of the entry and increments `hits`; a miss
    /// increments `misses`. Never panics, never performs I/O.
    pub fn get(&mut self, tool: &str, args_key: &str) -> Option<&str> {
        let key = Self::make_key(tool, args_key);
        if !self.entries.contains_key(&key) {
            self.misses += 1;
            return None;
        }
        self.clock += 1;
        let seq = self.clock;
        self.hits += 1;
        let entry = self.entries.get_mut(&key).expect("entry checked above");
        entry.seq = seq;
        Some(entry.output.as_str())
    }

    /// Inserts or refreshes an output.
    ///
    /// * existing key: the output and hint are replaced in place, the entry
    ///   becomes the most recently used one and no eviction happens;
    /// * new key at capacity: the least recently used entry is evicted first;
    /// * `max_entries == 0`: no-op.
    pub fn insert(
        &mut self,
        tool: &str,
        args_key: &str,
        output: impl Into<String>,
        path_hint: Option<&str>,
    ) {
        if self.max_entries == 0 {
            return;
        }
        let key = Self::make_key(tool, args_key);
        let hint = path_hint.map(normalize_path);
        self.clock += 1;
        let seq = self.clock;

        if self.entries.contains_key(&key) {
            let previous_hint = {
                let entry = self.entries.get_mut(&key).expect("entry checked above");
                entry.output = output.into();
                entry.seq = seq;
                std::mem::replace(&mut entry.path_hint, hint.clone())
            };
            self.unindex(&key, previous_hint.as_deref());
            self.index(&key, hint.as_deref());
            return;
        }

        if self.entries.len() >= self.max_entries {
            self.evict_lru();
        }
        let entry = Entry {
            output: output.into(),
            path_hint: hint.clone(),
            seq,
        };
        self.entries.insert(key.clone(), entry);
        self.index(&key, hint.as_deref());
    }

    /// Removes every entry that may have become stale after a write on `path`.
    ///
    /// An entry is dropped when its hint and `path` are the same, when one is an
    /// ancestor of the other, or when one of them is the project root (`""`,
    /// `"."` or `"./"`). Entries without a hint are only removed by
    /// [`clear`](Self::clear).
    pub fn invalidate_path(&mut self, path: &str) {
        let target = normalize_path(path);
        let mut victims: Vec<String> = Vec::new();
        for (hint, keys) in self.path_index.iter() {
            if paths_overlap(hint, &target) {
                victims.extend(keys.iter().cloned());
            }
        }
        for key in victims {
            self.remove_key(&key);
        }
    }

    /// Drops every entry but keeps the cumulative `hits`/`misses` telemetry.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.path_index.clear();
    }

    /// Telemetry snapshot. Side effect free.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits,
            misses: self.misses,
            len: self.entries.len(),
        }
    }

    fn make_key(tool: &str, args_key: &str) -> String {
        debug_assert!(!tool.contains(SEP), "tool must not contain U+001F");
        debug_assert!(!args_key.contains(SEP), "args_key must not contain U+001F");
        let mut key = String::with_capacity(tool.len() + args_key.len() + SEP.len_utf8());
        key.push_str(tool);
        key.push(SEP);
        key.push_str(args_key);
        key
    }

    fn index(&mut self, key: &str, hint: Option<&str>) {
        if let Some(hint) = hint {
            self.path_index
                .entry(hint.to_string())
                .or_insert_with(HashSet::new)
                .insert(key.to_string());
        }
    }

    fn unindex(&mut self, key: &str, hint: Option<&str>) {
        if let Some(hint) = hint {
            let empty = match self.path_index.get_mut(hint) {
                Some(keys) => {
                    keys.remove(key);
                    keys.is_empty()
                }
                None => false,
            };
            if empty {
                self.path_index.remove(hint);
            }
        }
    }

    fn remove_key(&mut self, key: &str) {
        let removed = self.entries.remove(key);
        if let Some(entry) = removed {
            self.unindex(key, entry.path_hint.as_deref());
        }
    }

    fn evict_lru(&mut self) {
        let victim = self
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.seq)
            .map(|(key, _)| key.clone());
        if let Some(victim) = victim {
            self.remove_key(&victim);
        }
    }
}

fn normalize_path(path: &str) -> String {
    let mut normalized = String::with_capacity(path.len());
    for segment in path.replace('\\', "/").split('/') {
        let segment = segment.trim();
        if segment.is_empty() || segment == "." {
            continue;
        }
        if !normalized.is_empty() {
            normalized.push('/');
        }
        normalized.push_str(segment);
    }
    normalized
}

fn is_ancestor(ancestor: &str, descendant: &str) -> bool {
    descendant.len() > ancestor.len()
        && descendant.starts_with(ancestor)
        && descendant.as_bytes()[ancestor.len()] == b'/'
}

fn paths_overlap(hint: &str, target: &str) -> bool {
    if hint.is_empty() || target.is_empty() {
        return true;
    }
    hint == target || is_ancestor(hint, target) || is_ancestor(target, hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_handles_separators_and_root() {
        assert_eq!(normalize_path(""), "");
        assert_eq!(normalize_path("."), "");
        assert_eq!(normalize_path("./"), "");
        assert_eq!(normalize_path("src\\a.rs"), "src/a.rs");
        assert_eq!(normalize_path("src/a.rs/"), "src/a.rs");
        assert_eq!(normalize_path("./src//nested/./b.rs"), "src/nested/b.rs");
    }

    #[test]
    fn overlap_detects_ancestry_both_ways() {
        assert!(paths_overlap("src", "src/a.rs"));
        assert!(paths_overlap("src/a.rs", "src"));
        assert!(paths_overlap("src/a.rs", "src/a.rs"));
        assert!(paths_overlap("", "src/a.rs"));
        assert!(!paths_overlap("src/a.rs", "src/ab.rs"));
    }
}
