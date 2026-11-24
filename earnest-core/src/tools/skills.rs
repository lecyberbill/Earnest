use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const BUILTIN_WFL_SKILL: &str = include_str!("../../../.gemini/skills/earnest-wfl/SKILL.md");

#[derive(Debug, Clone)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub source: String,
    pub content: String,
}

fn parse_frontmatter(raw: &str, fallback_name: &str) -> (String, String) {
    let mut name = fallback_name.to_string();
    let mut desc = String::new();
    let trimmed = raw.trim_start();
    if let Some(rest) = trimmed.strip_prefix("---") {
        if let Some(end) = rest.find("---") {
            for line in rest[..end].lines() {
                if let Some((k, v)) = line.split_once(':') {
                    let key = k.trim();
                    let val = v.trim().trim_matches('"').trim_matches('\'');
                    if key == "name" && !val.is_empty() {
                        name = val.to_string();
                    } else if key == "description" && !val.is_empty() {
                        desc = val.to_string();
                    }
                }
            }
        }
    }
    (name, desc)
}

pub fn discover_skills() -> Vec<SkillEntry> {
    let mut map: BTreeMap<String, SkillEntry> = BTreeMap::new();

    let (wfl_name, wfl_desc) = parse_frontmatter(BUILTIN_WFL_SKILL, "earnest-wfl");
    map.insert(
        wfl_name.clone(),
        SkillEntry {
            name: wfl_name,
            description: wfl_desc,
            source: "builtin".to_string(),
            content: BUILTIN_WFL_SKILL.to_string(),
        },
    );

    for root in [".gemini/skills", ".earnest/skills"] {
        let dir = Path::new(root);
        let Ok(rd) = fs::read_dir(dir) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            let skill_file = p.join("SKILL.md");
            let Ok(content) = fs::read_to_string(&skill_file) else { continue };
            let folder = entry.file_name().to_string_lossy().to_string();
            let (name, description) = parse_frontmatter(&content, &folder);
            map.insert(
                name.clone(),
                SkillEntry {
                    name,
                    description,
                    source: skill_file.to_string_lossy().replace('\\', "/"),
                    content,
                },
            );
        }
    }

    map.into_values().collect()
}

pub fn skills_prompt_summary() -> String {
    let skills = discover_skills();
    if skills.is_empty() {
        return String::new();
    }
    let items: Vec<String> = skills
        .into_iter()
        .map(|s| format!("{} ({})", s.name, s.description))
        .collect();
    format!(
        "Skills disponibles via `read_skill` / `list_skills` : {}. ",
        items.join(" ; ")
    )
}

pub fn list_skills_tool() -> Value {
    let skills: Vec<Value> = discover_skills()
        .into_iter()
        .map(|s| {
            json!({
                "name": s.name,
                "description": s.description,
                "source": s.source,
            })
        })
        .collect();
    json!({
        "status": "ok",
        "count": skills.len(),
        "skills": skills,
    })
}

pub fn read_skill_tool(name: &str) -> Result<Value, String> {
    let target = name.trim();
    if target.is_empty() {
        return Err("read_skill: nom de skill vide".to_string());
    }
    let skills = discover_skills();
    if let Some(found) = skills.into_iter().find(|s| s.name.eq_ignore_ascii_case(target)) {
        Ok(json!({
            "status": "ok",
            "name": found.name,
            "description": found.description,
            "source": found.source,
            "content": found.content,
        }))
    } else {
        Err(format!("Skill '{}' introuvable (appelle `list_skills` pour voir la liste)", target))
    }
}

fn global_earnest_dir() -> Option<std::path::PathBuf> {
    if let Ok(home) = std::env::var("EARNEST_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return Some(std::path::PathBuf::from(trimmed));
        }
    }
    #[cfg(windows)]
    if let Ok(appdata) = std::env::var("APPDATA") {
        return Some(std::path::PathBuf::from(appdata).join("Earnest"));
    }
    if let Ok(home) = std::env::var("HOME") {
        return Some(std::path::PathBuf::from(home).join(".earnest"));
    }
    None
}

pub fn load_earnest_md(workspace_root: &Path) -> String {
    let mut sections = Vec::new();
    if let Some(gdir) = global_earnest_dir() {
        let gfile = gdir.join("EARNEST.md");
        if let Ok(raw) = fs::read_to_string(&gfile) {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                let bounded: String = trimmed.chars().take(4_000).collect();
                sections.push(format!("[EARNEST.md GLOBAL ({})]\n{}", gfile.display(), bounded));
            }
        }
    }
    for rel in ["EARNEST.md", ".earnest/EARNEST.md"] {
        let p = workspace_root.join(rel);
        if let Ok(raw) = fs::read_to_string(&p) {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                let bounded: String = trimmed.chars().take(4_000).collect();
                sections.push(format!("[EARNEST.md PROJET ({})]\n{}", rel, bounded));
                break;
            }
        }
    }
    if sections.is_empty() {
        String::new()
    } else {
        format!("\n\n{}\n", sections.join("\n\n"))
    }
}
