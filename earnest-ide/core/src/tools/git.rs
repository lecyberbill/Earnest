use serde_json::Value;

pub fn get_git_status(root_dir: &str) -> Result<Value, String> {
    let output = crate::hidden_command("git")
        .args(["status", "--porcelain=v1", "-b"])
        .current_dir(root_dir)
        .output()
        .map_err(|e| format!("Erreur exécution git status: {}", e))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("Git error: {}", err));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let mut branch = String::from("detached");
    let mut files = Vec::new();

    for line in stdout.lines() {
        if line.starts_with("##") {
            // Extraction de la branche
            let branch_line = line.trim_start_matches("##").trim();
            if let Some(b) = branch_line.split("...").next() {
                branch = b.trim().to_string();
            }
        } else if line.len() >= 3 {
            let status_code = &line[0..2];
            let file_path = line[3..].trim().to_string();
            let state_str = match status_code.trim() {
                "M" => "modified",
                "A" => "added",
                "D" => "deleted",
                "??" => "untracked",
                "R" => "renamed",
                _ => "changed",
            };
            files.push(serde_json::json!({
                "path": file_path,
                "status": state_str,
                "raw_code": status_code.trim()
            }));
        }
    }

    Ok(serde_json::json!({
        "branch": branch,
        "files": files,
        "count": files.len()
    }))
}

pub fn get_git_diff(root_dir: &str, file_path: Option<&str>) -> Result<Value, String> {
    let mut cmd = crate::hidden_command("git");
    cmd.args(["diff", "HEAD"]);
    if let Some(fp) = file_path {
        if !fp.is_empty() {
            cmd.arg("--").arg(fp);
        }
    }
    cmd.current_dir(root_dir);

    let output = cmd.output().map_err(|e| format!("Erreur git diff: {}", e))?;
    let diff_text = String::from_utf8_lossy(&output.stdout).to_string();

    Ok(serde_json::json!({
        "file": file_path,
        "diff": diff_text
    }))
}

fn is_test_or_template_path(rel_path: &str) -> bool {
    let norm = rel_path.replace('\\', "/").to_ascii_lowercase();
    let base = norm.rsplit('/').next().unwrap_or(&norm);
    norm.starts_with("tests/")
        || norm.contains("/tests/")
        || base.starts_with("test_")
        || base.contains("_test.")
        || base.contains("_tests.")
        || matches!(
            base,
            ".env.example" | ".env.sample" | ".env.template" | ".env.dist"
        )
}

fn is_raw_dotenv_file(rel_path: &str) -> bool {
    let norm = rel_path.replace('\\', "/").to_ascii_lowercase();
    let base = norm.rsplit('/').next().unwrap_or(&norm);
    (base == ".env" || base.starts_with(".env.")) && !is_test_or_template_path(&norm)
}

/// Inspecte les fichiers indexés (`git diff --cached`) avec `dotenv_shield` avant un commit.
fn verify_no_staged_secrets(root_dir: &str) -> Result<(), String> {
    let names_out = crate::hidden_command("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(root_dir)
        .output()
        .map_err(|e| format!("Erreur git diff --cached: {}", e))?;
    let staged_files: Vec<String> = String::from_utf8_lossy(&names_out.stdout)
        .lines()
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect();

    for rel in &staged_files {
        if is_test_or_template_path(rel) {
            continue;
        }
        let full_path = std::path::Path::new(root_dir).join(rel);
        if is_raw_dotenv_file(rel) && full_path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                let findings = dotenv_shield::scan_secrets(&content);
                if !findings.is_empty() {
                    let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
                    let _ = crate::hidden_command("git")
                        .arg("reset")
                        .current_dir(root_dir)
                        .output();
                    return Err(format!(
                        "[DOTENV-SHIELD] Commit bloqué : fichier sensible '{}' contient {} secret(s) ({}). Ajoutez '{}' à .gitignore ou générez un .env.example.",
                        rel,
                        findings.len(),
                        kinds.join(", "),
                        rel
                    ));
                }
            }
        }

        let diff_out = crate::hidden_command("git")
            .args(["diff", "--cached", "-U0", "--", rel])
            .current_dir(root_dir)
            .output()
            .map_err(|e| format!("Erreur git diff sur {}: {}", rel, e))?;
        let diff_str = String::from_utf8_lossy(&diff_out.stdout);
        let added_lines: String = diff_str
            .lines()
            .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
            .map(|l| &l[1..])
            .collect::<Vec<_>>()
            .join("\n");
        let findings = dotenv_shield::scan_secrets(&added_lines);
        if !findings.is_empty() {
            let first = &findings[0];
            let _ = crate::hidden_command("git")
                .arg("reset")
                .current_dir(root_dir)
                .output();
            return Err(format!(
                "[DOTENV-SHIELD] Commit bloqué : secret détecté ({}) dans les lignes ajoutées de '{}'. Retirez le secret en clair avant de commiter.",
                first.kind.as_str(),
                rel
            ));
        }
    }
    Ok(())
}

pub fn git_commit(root_dir: &str, message: &str) -> Result<Value, String> {
    // 1. git add -A
    let add_out = crate::hidden_command("git")
        .args(["add", "-A"])
        .current_dir(root_dir)
        .output()
        .map_err(|e| format!("Erreur git add: {}", e))?;

    if !add_out.status.success() {
        return Err(String::from_utf8_lossy(&add_out.stderr).to_string());
    }

    // 2. Garde-fou pré-commit `dotenv_shield` contre la fuite de secrets
    verify_no_staged_secrets(root_dir)?;

    // 3. git commit -m
    let commit_out = crate::hidden_command("git")
        .args(["commit", "-m", message])
        .current_dir(root_dir)
        .output()
        .map_err(|e| format!("Erreur git commit: {}", e))?;

    let stdout = String::from_utf8_lossy(&commit_out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&commit_out.stderr).to_string();

    if commit_out.status.success() {
        Ok(serde_json::json!({
            "status": "success",
            "output": stdout
        }))
    } else {
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_commit_blocks_plaintext_secret_leak() {
        let tmp = std::env::temp_dir().join(format!("earnest_git_shield_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let root = tmp.to_string_lossy().to_string();

        crate::hidden_command("git").arg("init").current_dir(&tmp).output().unwrap();
        crate::hidden_command("git").args(["config", "user.email", "test@example.com"]).current_dir(&tmp).output().unwrap();
        crate::hidden_command("git").args(["config", "user.name", "Test"]).current_dir(&tmp).output().unwrap();

        std::fs::write(tmp.join(".env"), "OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz012345\n").unwrap();
        let err = git_commit(&root, "leak test").expect_err("secret commit must be blocked");
        assert!(err.contains("DOTENV-SHIELD"));

        std::fs::remove_file(tmp.join(".env")).unwrap();
        std::fs::write(tmp.join("README.md"), "# Safe Repo\n").unwrap();
        assert!(git_commit(&root, "clean commit").is_ok());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}

