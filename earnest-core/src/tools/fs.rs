use serde_json::{json, Value};
use std::fs;
use std::path::Path;

const IGNORED_DIRS: &[&str] = &[".git", "target", "node_modules", ".next", "dist", "build"];

fn guard_path(raw: &str) -> Result<&Path, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Chemin vide refusé".to_string());
    }
    let p = Path::new(trimmed);
    if tool_guard::path::is_system_sensitive(p) {
        return Err(tool_guard::error::GuardError::SystemPath {
            requested: trimmed.to_string(),
        }
        .to_string());
    }
    Ok(p)
}

pub fn read_file_tool(
    path_str: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> Result<Value, String> {
    let p = guard_path(path_str)?;
    let content = fs::read_to_string(p).map_err(|e| format!("read_file '{}': {}", path_str, e))?;
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    let start = start_line.unwrap_or(1).max(1);
    let max_end = (start + 399).min(total);
    let end = end_line.unwrap_or(max_end).min(max_end);
    let mut out = String::new();
    if start <= end && start <= total {
        for idx in start..=end {
            out.push_str(&format!("{}: {}\n", idx, lines[idx - 1]));
        }
    }
    Ok(json!({
        "status": "ok",
        "path": path_str,
        "start_line": start,
        "end_line": end,
        "total_lines": total,
        "truncated": end < total && end_line.is_none(),
        "content": out
    }))
}

pub fn edit_file_tool(path_str: &str, target: &str, replacement: &str) -> Result<Value, String> {
    let p = guard_path(path_str)?;
    let source = fs::read_to_string(p).map_err(|e| format!("edit_file '{}': {}", path_str, e))?;
    let lines_before = source.lines().count();
    let patched = atomic_patch::apply_patch(&source, target, replacement).map_err(|mr| {
        format!(
            "edit_file '{}' ({:?}): {}",
            path_str,
            mr.status,
            mr.context.unwrap_or_else(|| "bloc cible introuvable".to_string())
        )
    })?;
    let lines_after = patched.lines().count();
    fs::write(p, &patched).map_err(|e| format!("edit_file write '{}': {}", path_str, e))?;
    Ok(json!({
        "status": "ok",
        "path": path_str,
        "lines_before": lines_before,
        "lines_after": lines_after
    }))
}

pub fn list_dir_tool(path_str: &str, max_entries: usize) -> Result<Value, String> {
    let p = guard_path(path_str)?;
    let rd = fs::read_dir(p).map_err(|e| format!("list_dir '{}': {}", path_str, e))?;
    let mut entries = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if IGNORED_DIRS.contains(&name.as_str()) {
            continue;
        }
        let meta = entry.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        entries.push((is_dir, name, size));
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let total = entries.len();
    let items: Vec<Value> = entries
        .into_iter()
        .take(max_entries.clamp(1, 200))
        .map(|(is_dir, name, size)| {
            json!({
                "name": name,
                "kind": if is_dir { "dir" } else { "file" },
                "size": size
            })
        })
        .collect();
    Ok(json!({
        "status": "ok",
        "path": path_str,
        "total": total,
        "entries": items
    }))
}

pub fn grep_search_tool(
    query: &str,
    path_str: &str,
    max_results: usize,
    case_sensitive: bool,
) -> Result<Value, String> {
    if query.trim().is_empty() {
        return Err("grep_search: query vide".to_string());
    }
    let root = guard_path(path_str)?;
    let needle = if case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };
    let mut matches = Vec::new();
    let limit = max_results.clamp(1, 100);
    grep_walk(root, &needle, case_sensitive, limit, &mut matches);
    Ok(json!({
        "status": "ok",
        "query": query,
        "count": matches.len(),
        "matches": matches
    }))
}

fn grep_walk(
    dir: &Path,
    needle: &str,
    case_sensitive: bool,
    limit: usize,
    matches: &mut Vec<Value>,
) {
    if matches.len() >= limit {
        return;
    }
    if dir.is_file() {
        grep_file(dir, needle, case_sensitive, limit, matches);
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        if matches.len() >= limit {
            break;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if IGNORED_DIRS.contains(&name.as_str()) {
            continue;
        }
        let p = entry.path();
        if p.is_dir() {
            grep_walk(&p, needle, case_sensitive, limit, matches);
        } else if p.is_file() {
            grep_file(&p, needle, case_sensitive, limit, matches);
        }
    }
}

fn grep_file(
    path: &Path,
    needle: &str,
    case_sensitive: bool,
    limit: usize,
    matches: &mut Vec<Value>,
) {
    let Ok(meta) = fs::metadata(path) else { return };
    if meta.len() > 1_048_576 {
        return;
    }
    let Ok(text) = fs::read_to_string(path) else { return };
    for (idx, line) in text.lines().enumerate() {
        let hit = if case_sensitive {
            line.contains(needle)
        } else {
            line.to_lowercase().contains(needle)
        };
        if hit {
            let snippet: String = line.chars().take(240).collect();
            matches.push(json!({
                "file": path.to_string_lossy().replace('\\', "/"),
                "line": idx + 1,
                "text": snippet.trim_end()
            }));
            if matches.len() >= limit {
                break;
            }
        }
    }
}
