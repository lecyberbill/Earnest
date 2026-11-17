//! Integration tests for the public API of `tool_output_cache`.

use tool_output_cache::{CacheStats, ToolCache};

fn seeded(capacity: usize) -> ToolCache {
    let mut cache = ToolCache::new(capacity);
    cache.insert("read_file", "src/a.rs", "content-a", Some("src/a.rs"));
    cache
}

#[test]
fn cache_hit_returns_value_and_counts_hit() {
    let mut cache = seeded(8);
    assert_eq!(cache.get("read_file", "src/a.rs"), Some("content-a"));
    let stats = cache.stats();
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 0);
    assert_eq!(stats.len, 1);
    assert!(cache.contains("read_file", "src/a.rs"));
}

#[test]
fn cache_miss_returns_none_and_counts_miss() {
    let mut cache = ToolCache::new(4);
    assert!(cache.get("read_file", "missing.rs").is_none());
    let stats: CacheStats = cache.stats();
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.len, 0);
}

#[test]
fn distinct_tools_and_keys_are_isolated() {
    let mut cache = ToolCache::new(8);
    cache.insert("read_file", "a.rs", "content-a", Some("a.rs"));
    cache.insert("list_files", "a.rs", "listing-a", Some("a.rs"));
    cache.insert("read_file", "b.rs", "content-b", Some("b.rs"));

    assert_eq!(cache.len(), 3);
    assert_eq!(cache.get("read_file", "a.rs"), Some("content-a"));
    assert_eq!(cache.get("list_files", "a.rs"), Some("listing-a"));
    assert_eq!(cache.get("read_file", "b.rs"), Some("content-b"));
    assert!(cache.get("grep_search", "a.rs").is_none());
}

#[test]
fn evicts_least_recently_used_at_capacity() {
    let mut cache = ToolCache::new(2);
    cache.insert("read_file", "a.rs", "A", Some("a.rs"));
    cache.insert("read_file", "b.rs", "B", Some("b.rs"));

    // Touch a.rs so that b.rs becomes the least recently used entry.
    assert_eq!(cache.get("read_file", "a.rs"), Some("A"));
    cache.insert("read_file", "c.rs", "C", Some("c.rs"));

    assert_eq!(cache.len(), 2);
    assert_eq!(cache.get("read_file", "a.rs"), Some("A"));
    assert_eq!(cache.get("read_file", "c.rs"), Some("C"));
    assert!(!cache.contains("read_file", "b.rs"));
}

#[test]
fn capacity_never_exceeds_max_entries() {
    let mut cache = ToolCache::new(3);
    for index in 0..20 {
        let key = format!("key-{index}");
        cache.insert("read_file", &key, format!("value-{index}"), Some(&key));
        assert!(cache.len() <= cache.max_entries());
        assert_eq!(cache.stats().len, cache.len());
    }

    let mut disabled = ToolCache::new(0);
    disabled.insert("read_file", "a.rs", "A", Some("a.rs"));
    assert_eq!(disabled.len(), 0);
    assert!(disabled.get("read_file", "a.rs").is_none());
}

#[test]
fn invalidate_removes_file_entry_by_path_hint() {
    let mut cache = ToolCache::new(8);
    cache.insert("read_file", "src/a.rs", "A", Some("src/a.rs"));
    cache.insert("read_file", "src/b.rs", "B", Some("src/b.rs"));

    cache.invalidate_path("src/a.rs");

    assert_eq!(cache.len(), 1);
    assert!(cache.get("read_file", "src/a.rs").is_none());
    assert_eq!(cache.get("read_file", "src/b.rs"), Some("B"));
}

#[test]
fn invalidate_removes_directory_listing_and_grep_entries() {
    let mut cache = ToolCache::new(8);
    cache.insert("list_files", "src", "[a.rs, b.rs]", Some("src"));
    cache.insert("grep_search", "fn main", "src/a.rs:3:fn main() {}", Some("src"));
    cache.insert("read_file", "src/a.rs", "A", Some("src/a.rs"));
    cache.insert("read_file", "src/sub/deep.rs", "D", Some("src/sub/deep.rs"));

    cache.invalidate_path("src/a.rs");

    assert!(cache.get("list_files", "src").is_none());
    assert!(cache.get("grep_search", "fn main").is_none());
    assert!(cache.get("read_file", "src/a.rs").is_none());
    assert_eq!(cache.get("read_file", "src/sub/deep.rs"), Some("D"));
    assert_eq!(cache.len(), 1);
}

#[test]
fn invalidate_preserves_unrelated_entries() {
    let mut cache = ToolCache::new(8);
    cache.insert("read_file", "src/a.rs", "A", Some("src/a.rs"));
    cache.insert("read_file", "docs/readme.md", "R", Some("docs/readme.md"));
    cache.insert("list_files", "tests", "listing", Some("tests"));

    cache.invalidate_path("src/a.rs");

    assert_eq!(cache.len(), 2);
    assert_eq!(cache.get("read_file", "docs/readme.md"), Some("R"));
    assert_eq!(cache.get("list_files", "tests"), Some("listing"));
    assert_eq!(cache.stats().len, 2);
    assert!(!cache.contains("read_file", "src/a.rs"));
}

#[test]
fn clear_empties_cache_len_zero() {
    let mut cache = ToolCache::new(4);
    cache.insert("read_file", "src/a.rs", "A", Some("src/a.rs"));
    cache.insert("list_files", "src", "[a.rs]", Some("src"));
    assert_eq!(cache.get("read_file", "src/a.rs"), Some("A"));

    cache.clear();
    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());
    assert_eq!(cache.stats().len, 0);

    // Re-inserting after `clear` must not resurrect stale path index entries.
    cache.insert("read_file", "src/a.rs", "A2", Some("src/a.rs"));
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.get("read_file", "src/a.rs"), Some("A2"));

    cache.invalidate_path("src");
    assert_eq!(cache.len(), 0);
}

#[test]
fn stats_reflect_hits_misses_and_len() {
    let mut cache = ToolCache::new(4);
    cache.insert("read_file", "a.rs", "A", Some("a.rs"));

    let _ = cache.get("read_file", "a.rs");
    let _ = cache.get("read_file", "zz.rs");
    let _ = cache.get("read_file", "a.rs");

    let stats = cache.stats();
    assert_eq!(stats.hits, 2);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.len, 1);

    cache.clear();
    let after_clear = cache.stats();
    assert_eq!(after_clear.hits, 2);
    assert_eq!(after_clear.misses, 1);
    assert_eq!(after_clear.len, 0);
}

#[test]
fn manifest_declares_no_dependencies() {
    let manifest = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/Cargo.toml"
    ))
    .expect("Cargo.toml must be readable");
    assert!(!manifest.contains("[dependencies]"));
    assert!(!manifest.contains("[dev-dependencies]"));
    assert!(!manifest.contains("[build-dependencies]"));
}
