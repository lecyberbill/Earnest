use std::fs;
use std::path::PathBuf;
use symbol_index::{SymbolIndex, SymbolKind, read_line_range, read_symbol_snippet};

fn temp_dir(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("symbol_index_test_{}_{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn indexes_multi_language_symbols_and_skips_target() {
    let dir = temp_dir("multi_lang");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("target/debug")).unwrap();

    fs::write(
        dir.join("src/engine.rs"),
        "pub struct ReActEngine {\n    steps: usize,\n}\n\npub async fn run_react_loop() {}\nenum Status { Ok, Err }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/helper.py"),
        "class ContextBuilder:\n    pass\n\ndef build_prompt(ctx):\n    return ctx\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/api.ts"),
        "export interface RunConfig { id: string }\nexport async function streamEvents() {}\nexport const MAX_STEPS = 0;\n",
    )
    .unwrap();
    // Should be ignored
    fs::write(dir.join("target/debug/generated.rs"), "pub fn ignored_fn() {}\n").unwrap();

    let index = SymbolIndex::build(&dir).unwrap();
    assert!(index.search("ignored_fn", 5).is_empty());

    let react = index.search("ReActEngine", 5);
    assert_eq!(react.len(), 1);
    assert_eq!(react[0].kind, SymbolKind::Struct);
    assert_eq!(react[0].line, 1);

    let py_cls = index.search("ContextBuilder", 5);
    assert_eq!(py_cls[0].kind, SymbolKind::Class);

    let ts_fn = index.search("streamEvents", 5);
    assert_eq!(ts_fn[0].kind, SymbolKind::Function);

    let ts_exp = index.search("MAX_STEPS", 5);
    assert_eq!(ts_exp[0].kind, SymbolKind::Export);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fuzzy_search_ranks_exact_and_subsequence() {
    let dir = temp_dir("fuzzy");
    fs::write(
        dir.join("lib.rs"),
        "fn compute_alpha() {}\nfn parse_token_budget() {}\nfn parse_tokens() {}\n",
    )
    .unwrap();

    let index = SymbolIndex::build(&dir).unwrap();
    let exact = index.search("parse_tokens", 5);
    assert_eq!(exact[0].name, "parse_tokens");

    let fuzzy = index.search("ptb", 5);
    assert!(!fuzzy.is_empty());
    assert_eq!(fuzzy[0].name, "parse_token_budget");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn targeted_line_range_reader_bounds_output() {
    let dir = temp_dir("reader");
    let file = dir.join("sample.rs");
    let lines: Vec<String> = (1..=50).map(|i| format!("let line_{} = {};", i, i)).collect();
    fs::write(&file, lines.join("\n")).unwrap();

    let snip = read_line_range(&file, 10, 12).unwrap();
    assert_eq!(snip.start_line, 10);
    assert_eq!(snip.end_line, 12);
    assert_eq!(snip.total_lines, 50);
    assert!(snip.content.contains("L10: let line_10 = 10;"));
    assert!(snip.content.contains("L12: let line_12 = 12;"));
    assert!(!snip.content.contains("L13:"));

    let ctx = read_symbol_snippet(&file, 25, 2, 2).unwrap();
    assert_eq!(ctx.start_line, 23);
    assert_eq!(ctx.end_line, 27);

    let _ = fs::remove_dir_all(&dir);
}
