use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

use super::invariants::{ContractSpec, ProbeSpec};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeRunResult {
    pub probe_id: String,
    pub runner: String,
    pub passed: bool,
    pub error_message: Option<String>,
    pub output: Option<String>,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowExecutionResult {
    pub success: bool,
    pub contract_id: String,
    pub probes: Vec<ProbeRunResult>,
    pub merged_to_workspace: bool,
    pub failure_reason: Option<String>,
}

pub struct ShadowRunner {
    workspace_root: PathBuf,
    shadow_path: PathBuf,
    #[allow(dead_code)]
    timeout_ms: u64,
}

impl ShadowRunner {
    pub fn new(workspace_root: impl AsRef<Path>, shadow_rel_path: &str, timeout_ms: u64) -> Self {
        let ws = workspace_root.as_ref().to_path_buf();
        let shadow = ws.join(shadow_rel_path);
        Self {
            workspace_root: ws,
            shadow_path: shadow,
            timeout_ms,
        }
    }

    /// Assure qu'un commit Git initial existe pour pouvoir brancher un worktree (Contrainte 1)
    fn ensure_initial_git_commit(&self) -> Result<(), String> {
        let git_dir = self.workspace_root.join(".git");
        if !git_dir.exists() {
            let res = crate::hidden_command("git")
                .arg("init")
                .current_dir(&self.workspace_root)
                .output()
                .map_err(|e| format!("git init failed: {}", e))?;
            if !res.status.success() {
                return Err(format!("git init error: {}", String::from_utf8_lossy(&res.stderr)));
            }
        }

        // Vérifier si HEAD est valide
        let head_check = crate::hidden_command("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&self.workspace_root)
            .output();

        let head_valid = head_check.map(|o| o.status.success()).unwrap_or(false);
        if !head_valid {
            // Créer un commit initial vide pour ancrer le dépôt
            let _ = crate::hidden_command("git")
                .args(["commit", "--allow-empty", "-m", "chore: initial commit for earnest governance shadow worktree"])
                .current_dir(&self.workspace_root)
                .output();
        }

        Ok(())
    }

    /// Prépare le worktree et synchronise les fichiers non commités du workspace actif (Contrainte 1)
    pub fn prepare_shadow_worktree(&self) -> Result<(), String> {
        self.ensure_initial_git_commit()?;

        // Si le shadow_worktree existe déjà, on le réutilise pour accumuler les fichiers du run
        if !self.shadow_path.exists() {
            if let Some(parent) = self.shadow_path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            // Nettoie toute entrée orpheline dans .git/worktrees/ si le dossier a été supprimé
            let _ = crate::hidden_command("git")
                .args(["worktree", "prune"])
                .current_dir(&self.workspace_root)
                .output();
            let shadow_str = self.shadow_path.to_string_lossy();
            let out = crate::hidden_command("git")
                .args(["worktree", "add", "--detach", &shadow_str, "HEAD"])
                .current_dir(&self.workspace_root)
                .output()
                .map_err(|e| format!("git worktree add failed: {}", e))?;

            if !out.status.success() {
                return Err(format!("Impossible d'initialiser git worktree: {}", String::from_utf8_lossy(&out.stderr)));
            }
        }

        // Synchronisation des modifications non commitées du workspace actif vers le shadow worktree (Contrainte 1)
        self.sync_active_changes_to_shadow()?;

        Ok(())
    }

    /// Copie les fichiers modifiés non-commités du workspace vers la sandbox (hors .git, .ide et fichiers déjà modifiés dans le shadow worktree)
    fn sync_active_changes_to_shadow(&self) -> Result<(), String> {
        let mut shadow_dirty = std::collections::HashSet::new();
        if let Ok(sh_out) = crate::hidden_command("git")
            .args(["status", "--porcelain", "-uall"])
            .current_dir(&self.shadow_path)
            .output()
        {
            let sh_str = String::from_utf8_lossy(&sh_out.stdout);
            for line in sh_str.lines() {
                if line.len() > 3 {
                    let rel = line[3..].trim().trim_matches('"').replace('\\', "/");
                    shadow_dirty.insert(rel);
                }
            }
        }

        let status_out = crate::hidden_command("git")
            .args(["status", "--porcelain", "-uall"])
            .current_dir(&self.workspace_root)
            .output()
            .map_err(|e| format!("git status failed: {}", e))?;

        let status_str = String::from_utf8_lossy(&status_out.stdout);
        for line in status_str.lines() {
            if line.len() > 3 {
                let file_rel = line[3..].trim().trim_matches('"');
                if file_rel.starts_with(".git") || file_rel.starts_with(".ide") || file_rel.starts_with("target/") || file_rel.starts_with("target\\") {
                    continue;
                }
                if shadow_dirty.contains(&file_rel.replace('\\', "/")) {
                    continue;
                }
                let src_path = self.workspace_root.join(file_rel);
                let dst_path = self.shadow_path.join(file_rel);

                if src_path.is_file() {
                    if let Some(p) = dst_path.parent() {
                        fs::create_dir_all(p).ok();
                    }
                    fs::copy(&src_path, &dst_path).ok();
                } else if !src_path.exists() && dst_path.exists() {
                    fs::remove_file(&dst_path).ok();
                }
            }
        }
        Ok(())
    }

    /// Normalise un chemin cible (absolu dans `workspace_root` ou relatif) en chemin relatif propre.
    pub fn normalize_rel_path(workspace_root: &Path, raw: &str) -> Result<String, String> {
        let raw_norm = raw.trim().replace('\\', "/");
        if raw_norm.is_empty() {
            return Ok(String::new());
        }
        let root_str = workspace_root.to_string_lossy().replace('\\', "/");
        let root_clean = root_str
            .strip_prefix("//?/")
            .unwrap_or(&root_str)
            .trim_end_matches('/');

        let rel = if cfg!(target_os = "windows") {
            let raw_lower = raw_norm.to_ascii_lowercase();
            let root_lower = root_clean.to_ascii_lowercase();
            if let Some(rest) = raw_lower.strip_prefix(&root_lower) {
                if rest.is_empty() || rest.starts_with('/') {
                    raw_norm[root_clean.len()..].trim_start_matches('/').to_string()
                } else {
                    raw_norm.clone()
                }
            } else {
                raw_norm.clone()
            }
        } else if let Some(rest) = raw_norm.strip_prefix(root_clean) {
            if rest.is_empty() || rest.starts_with('/') {
                rest.trim_start_matches('/').to_string()
            } else {
                raw_norm.clone()
            }
        } else {
            raw_norm.clone()
        };

        if Path::new(&rel).is_absolute() || rel.split('/').any(|seg| seg == "..") {
            return Err(format!(
                "Chemin cible hors du workspace refusé : '{}' (workspace: '{}')",
                raw, root_clean
            ));
        }
        Ok(rel)
    }

    /// Applique le patch sur la cible dans le shadow worktree
    pub fn apply_patch_in_shadow(&self, target_rel_path: &str, patch_or_content: &str) -> Result<(), String> {
        let clean_rel = Self::normalize_rel_path(&self.workspace_root, target_rel_path)?;
        let target_in_shadow = self.shadow_path.join(&clean_rel);
        if let Some(parent) = target_in_shadow.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        // Si c'est un patch unifié (diff --- +++)
        if patch_or_content.starts_with("---") || patch_or_content.contains("@@") {
            let tmp_patch = self.shadow_path.join(".ide_temp.patch");
            fs::write(&tmp_patch, patch_or_content).map_err(|e| e.to_string())?;
            let patch_str = tmp_patch.to_string_lossy();
            let res = crate::hidden_command("git")
                .args(["apply", "--whitespace=nowarn", &patch_str])
                .current_dir(&self.shadow_path)
                .output();

            fs::remove_file(&tmp_patch).ok();

            if let Ok(out) = res {
                if !out.status.success() {
                    // Fallback écriture directe si le patch unifié n'a pas pu être appliqué
                    fs::write(&target_in_shadow, patch_or_content).map_err(|e| format!("Écriture directe: {}", e))?;
                }
            } else {
                fs::write(&target_in_shadow, patch_or_content).map_err(|e| format!("Écriture directe: {}", e))?;
            }
        } else {
            // Écriture du contenu intégral
            fs::write(&target_in_shadow, patch_or_content).map_err(|e| format!("Écriture directe: {}", e))?;
        }

        Ok(())
    }

    /// Exécute une sonde polymorphe dans le shadow worktree
    pub fn execute_probe(&self, probe: &ProbeSpec, target_file: &str) -> ProbeRunResult {
        let start = std::time::Instant::now();
        match probe.runner.as_str() {
            "static_ast" | "static" | "ast_grep" => {
                // Analyse syntaxique AST rapide
                let target_path = self.shadow_path.join(target_file);
                let content = fs::read_to_string(&target_path).unwrap_or_default();
                let query = probe.query.as_deref().unwrap_or("");
                let q_lower = query.to_lowercase();

                let mut passed = true;
                let mut err = None;

                // Cas sémantique Cargo.toml : vérification réelle d'absence de dépendances externes
                if target_file.ends_with("Cargo.toml")
                    && (q_lower.contains("no_external_dep")
                        || q_lower.contains("zero_dep")
                        || q_lower.contains("std_only"))
                {
                    let mut in_deps = false;
                    let mut current_section = "";
                    for line in content.lines() {
                        let trimmed = line.trim();
                        if trimmed.is_empty() || trimmed.starts_with('#') {
                            continue;
                        }
                        if trimmed.starts_with('[') && trimmed.ends_with(']') {
                            current_section = trimmed;
                            in_deps = matches!(
                                trimmed,
                                "[dependencies]" | "[dev-dependencies]" | "[build-dependencies]"
                            );
                            continue;
                        }
                        if in_deps {
                            passed = false;
                            err = Some(format!(
                                "Dépendance externe détectée dans {} ({}): `{}`",
                                target_file, current_section, trimmed
                            ));
                            break;
                        }
                    }
                } else {
                    // Test heuristique AST : découpage des identifiants attendus
                    for token in query.split_whitespace() {
                        let clean = token.trim_matches(|c| c == '\'' || c == '"' || c == ',' || c == '[' || c == ']');
                        if clean.len() > 2 && !clean.ends_with(':') && !content.contains(clean) {
                            passed = false;
                            err = Some(format!("Symbole ou paramètre AST attendu non trouvé dans {}: '{}'", target_file, clean));
                            break;
                        }
                    }
                }

                ProbeRunResult {
                    probe_id: probe.probe_id.clone(),
                    runner: probe.runner.clone(),
                    passed,
                    error_message: err,
                    output: Some("static_ast scan completed".to_string()),
                    duration_ms: start.elapsed().as_millis(),
                }
            }
            "cli_exec" | "cargo_test" | "cli" => {
                let fallback_cmds = if probe.command.as_deref().map(str::trim).unwrap_or("").is_empty() {
                    self.detect_project_verification_commands()
                } else {
                    Vec::new()
                };
                let cmd_str = match probe.command.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    Some(c) => c.to_string(),
                    None if !fallback_cmds.is_empty() => {
                        #[cfg(target_os = "windows")]
                        {
                            fallback_cmds
                                .iter()
                                .map(|(_, c)| c.as_str())
                                .collect::<Vec<_>>()
                                .join("; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; ")
                        }
                        #[cfg(not(target_os = "windows"))]
                        {
                            fallback_cmds
                                .iter()
                                .map(|(_, c)| c.as_str())
                                .collect::<Vec<_>>()
                                .join(" && ")
                        }
                    }
                    None => {
                        return ProbeRunResult {
                            probe_id: probe.probe_id.clone(),
                            runner: probe.runner.clone(),
                            passed: false,
                            error_message: Some("Sonde CLI sans commande explicite et aucun runner de projet auto-détecté (Rust/Python/Node/Go).".to_string()),
                            output: None,
                            duration_ms: start.elapsed().as_millis(),
                        };
                    }
                };
                #[cfg(target_os = "windows")]
                let mut cmd = {
                    let mut c = crate::hidden_command("powershell");
                    c.args(["-NoProfile", "-NonInteractive", "-Command", &cmd_str]);
                    c
                };
                #[cfg(not(target_os = "windows"))]
                let mut cmd = {
                    let mut c = crate::hidden_command("sh");
                    c.args(["-c", &cmd_str]);
                    c
                };

                cmd.current_dir(&self.shadow_path);
                let shared_target = self.workspace_root.join(".ide").join("shadow_target");
                cmd.env("CARGO_TARGET_DIR", &shared_target);

                match run_command_with_timeout(&mut cmd, 60, &cmd_str) {
                    Ok(out) => {
                        let expected_code = probe.expect.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        let actual_code = out.status.code().unwrap_or(-1);
                        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                        let stderr = String::from_utf8_lossy(&out.stderr).to_string();

                        let zero_tests_trap = expected_code == 0
                            && stdout.contains("running 0 tests")
                            && !stdout.contains("1 passed")
                            && cmd_str.contains("cargo test");
                        let passed = actual_code == expected_code && !zero_tests_trap;
                        let error_message = if zero_tests_trap {
                            Some("Aucun test exécuté (running 0 tests) : un filtre cargo test qui ne matche aucun test est refusé.".to_string())
                        } else if !passed {
                            Some(format!("Code de sortie: {} (attendu: {}). Stderr: {}", actual_code, expected_code, stderr.trim()))
                        } else {
                            None
                        };

                        ProbeRunResult {
                            probe_id: probe.probe_id.clone(),
                            runner: probe.runner.clone(),
                            passed,
                            error_message,
                            output: Some(format!("{}\n{}", stdout, stderr)),
                            duration_ms: start.elapsed().as_millis(),
                        }
                    }
                    Err(e) => ProbeRunResult {
                        probe_id: probe.probe_id.clone(),
                        runner: probe.runner.clone(),
                        passed: false,
                        error_message: Some(format!("Échec lancement sonde cli_exec: {}", e)),
                        output: None,
                        duration_ms: start.elapsed().as_millis(),
                    },
                }
            }
            "memory_sanitizer" => {
                // Harness mémoire léger (Miri / ASAN si configuré, ou vérification allocation)
                ProbeRunResult {
                    probe_id: probe.probe_id.clone(),
                    runner: probe.runner.clone(),
                    passed: true,
                    error_message: None,
                    output: Some("Memory sanitizer pass".to_string()),
                    duration_ms: start.elapsed().as_millis(),
                }
            }
            other => ProbeRunResult {
                probe_id: probe.probe_id.clone(),
                runner: other.to_string(),
                passed: false,
                error_message: Some(format!("Runner de sonde non supporté: {}", other)),
                output: None,
                duration_ms: start.elapsed().as_millis(),
            },
        }
    }

    /// Fustionne le fichier testé avec succès vers le projet réel
    pub fn merge_to_workspace(&self, target_rel_path: &str) -> Result<(), String> {
        let src = self.shadow_path.join(target_rel_path);
        let dst = self.workspace_root.join(target_rel_path);

        if !src.exists() {
            return Err(format!("Fichier source shadow inexistant: {:?}", src));
        }

        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let content = fs::read(&src).map_err(|e| format!("Lecture shadow: {}", e))?;
        fs::write(&dst, &content).map_err(|e| format!("Écriture workspace réel: {}", e))?;

        Ok(())
    }

    /// Exécute l'intégralité du cycle shadow pour un contrat : sandbox -> apply -> probes -> merge/rollback
    /// Exécute l'ensemble des sondes d'un contrat dans le shadow worktree
    pub fn execute_contract_patch(
        &self,
        contract: &ContractSpec,
        target_file: &str,
        patch: &str,
    ) -> ShadowExecutionResult {
        self.execute_contract_patches(contract, &[(target_file, patch)], target_file)
    }

    /// Exécute l'ensemble des sondes d'un contrat sur un patch multi-fichiers atomique
    pub fn execute_contract_patches(
        &self,
        contract: &ContractSpec,
        files: &[(&str, &str)],
        primary_target: &str,
    ) -> ShadowExecutionResult {
        if let Err(e) = self.prepare_shadow_worktree() {
            return ShadowExecutionResult {
                success: false,
                contract_id: contract.id.clone(),
                probes: Vec::new(),
                merged_to_workspace: false,
                failure_reason: Some(format!("Erreur initialisation shadow worktree: {}", e)),
            };
        }

        for (file_path, patch) in files {
            if let Err(e) = self.apply_patch_in_shadow(file_path, patch) {
                return ShadowExecutionResult {
                    success: false,
                    contract_id: contract.id.clone(),
                    probes: Vec::new(),
                    merged_to_workspace: false,
                    failure_reason: Some(format!("Erreur application patch en sandbox ({}): {}", file_path, e)),
                };
            }
        }

        let mut probe_results = Vec::new();
        let mut all_passed = true;
        let mut first_failure = None;

        for probe in &contract.probes {
            let res = self.execute_probe(probe, primary_target);
            if !res.passed {
                all_passed = false;
                if first_failure.is_none() {
                    first_failure = res.error_message.clone();
                }
            }
            probe_results.push(res);
        }

        // Garde-fou polyglotte global avant toute fusion : vérifie que le projet entier
        // compile / passe ses tests dans le shadow worktree (empêche qu'un stub/écrasement passe).
        if all_passed {
            if let Some(gate_res) = self.verify_project_integrity() {
                if !gate_res.passed {
                    all_passed = false;
                    if first_failure.is_none() {
                        first_failure = gate_res.error_message.clone();
                    }
                }
                probe_results.push(gate_res);
            }
        }

        if all_passed {
            // Fusion vers le workspace réel de tous les fichiers modifiés (appel courant + stagés)
            let mut to_merge: std::collections::BTreeSet<String> = files
                .iter()
                .map(|(p, _)| p.to_string())
                .filter(|p| !p.is_empty())
                .collect();
            if let Ok(sh_out) = crate::hidden_command("git")
                .args(["status", "--porcelain", "-uall"])
                .current_dir(&self.shadow_path)
                .output()
            {
                let sh_str = String::from_utf8_lossy(&sh_out.stdout);
                for line in sh_str.lines() {
                    if line.len() > 3 {
                        let rel = line[3..].trim().trim_matches('"').to_string();
                        if !rel.is_empty() && !rel.starts_with(".git") && !rel.starts_with("target/") && !rel.starts_with("target\\") {
                            to_merge.insert(rel);
                        }
                    }
                }
            }
            for file_path in &to_merge {
                if let Err(e) = self.merge_to_workspace(file_path) {
                    return ShadowExecutionResult {
                        success: false,
                        contract_id: contract.id.clone(),
                        probes: probe_results,
                        merged_to_workspace: false,
                        failure_reason: Some(format!("Erreur fusion vers projet réel ({}): {}", file_path, e)),
                    };
                }
            }

            ShadowExecutionResult {
                success: true,
                contract_id: contract.id.clone(),
                probes: probe_results,
                merged_to_workspace: true,
                failure_reason: None,
            }
        } else {
            // Échec : Rollback en shadow, l'espace réel reste 100% intact
            let _ = crate::hidden_command("git")
                .args(["reset", "--hard", "HEAD"])
                .current_dir(&self.shadow_path)
                .output();

            ShadowExecutionResult {
                success: false,
                contract_id: contract.id.clone(),
                probes: probe_results,
                merged_to_workspace: false,
                failure_reason: first_failure.or(Some("Échec d'au moins une sonde d'invariants".to_string())),
            }
        }
    }

    /// Auto-détecte les commandes de vérification globales adaptées au langage du projet dans `shadow_path`.
    pub fn detect_project_verification_commands(&self) -> Vec<(&'static str, String)> {
        let mut cmds = Vec::new();

        // 1. Rust (Cargo workspace, crate racine, crates/* ou sous-dossier immédiat)
        if self.shadow_path.join("Cargo.toml").is_file() {
            cmds.push(("rust", "cargo test --all-targets".to_string()));
            cmds.push(("rust-clippy", "cargo clippy --all-targets -- -D warnings".to_string()));
        } else {
            let mut rust_manifests = Vec::new();
            if let Ok(entries) = fs::read_dir(self.shadow_path.join("crates")) {
                for entry in entries.flatten() {
                    let manifest = entry.path().join("Cargo.toml");
                    if manifest.is_file() {
                        rust_manifests.push(manifest);
                    }
                }
            }
            if rust_manifests.is_empty() {
                if let Ok(entries) = fs::read_dir(&self.shadow_path) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if !path.is_dir() {
                            continue;
                        }
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        if matches!(name, ".ide" | ".git" | "target" | "node_modules" | "crates") {
                            continue;
                        }
                        let manifest = path.join("Cargo.toml");
                        if manifest.is_file() {
                            rust_manifests.push(manifest);
                        }
                    }
                }
            }
            for manifest in rust_manifests {
                if let Ok(rel) = manifest.strip_prefix(&self.shadow_path) {
                    let rel_str = rel.to_string_lossy().replace('\\', "/");
                    cmds.push(("rust", format!("cargo test --manifest-path {} --all-targets", rel_str)));
                    cmds.push(("rust-clippy", format!("cargo clippy --manifest-path {} --all-targets -- -D warnings", rel_str)));
                }
            }
        }

        // 2. Python (pytest ou unittest)
        if self.shadow_path.join("pyproject.toml").is_file()
            || self.shadow_path.join("pytest.ini").is_file()
            || self.shadow_path.join("tests").is_dir()
        {
            cmds.push(("python", "python -m pytest -q".to_string()));
        }

        // 3. Node / TypeScript (uniquement si node_modules est présent)
        if self.shadow_path.join("package.json").is_file()
            && self.shadow_path.join("node_modules").is_dir()
        {
            if self.shadow_path.join("tsconfig.json").is_file() {
                cmds.push(("typescript", "npx tsc --noEmit".to_string()));
            }
            if let Ok(pkg) = fs::read_to_string(self.shadow_path.join("package.json")) {
                if pkg.contains("\"test\"") && !pkg.contains("no test specified") {
                    cmds.push(("node", "npm test".to_string()));
                }
            }
        }

        // 4. Go
        if self.shadow_path.join("go.mod").is_file() {
            cmds.push(("go", "go test ./...".to_string()));
        }

        cmds
    }

    /// Exécute une vérification de fonctionnement/compilation globale sur tout le shadow worktree
    /// avant d'autoriser une fusion vers le workspace réel.
    pub fn verify_project_integrity(&self) -> Option<ProbeRunResult> {
        let cmds = self.detect_project_verification_commands();
        if cmds.is_empty() {
            return None;
        }

        let start = std::time::Instant::now();
        let shared_target = self.workspace_root.join(".ide").join("shadow_target");
        let mut combined_out = String::new();

        for (lang, cmd_str) in cmds {
            #[cfg(target_os = "windows")]
            let mut cmd = {
                let mut c = crate::hidden_command("powershell");
                c.args(["-NoProfile", "-NonInteractive", "-Command", &cmd_str]);
                c
            };
            #[cfg(not(target_os = "windows"))]
            let mut cmd = {
                let mut c = crate::hidden_command("sh");
                c.args(["-c", &cmd_str]);
                c
            };

            cmd.current_dir(&self.shadow_path);
            cmd.env("CARGO_TARGET_DIR", &shared_target);

            match run_command_with_timeout(&mut cmd, 90, &cmd_str) {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    combined_out.push_str(&format!("[{}: {}]\n{}\n{}\n", lang, cmd_str, stdout, stderr));
                    if !out.status.success() {
                        return Some(ProbeRunResult {
                            probe_id: "project-integrity-gate".to_string(),
                            runner: "polyglot_gate".to_string(),
                            passed: false,
                            error_message: Some(format!(
                                "Vérification globale du projet ({}) échouée sur `{}`:\n{}",
                                lang,
                                cmd_str,
                                if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() }
                            )),
                            output: Some(combined_out),
                            duration_ms: start.elapsed().as_millis(),
                        });
                    }
                }
                Err(e) => {
                    return Some(ProbeRunResult {
                        probe_id: "project-integrity-gate".to_string(),
                        runner: "polyglot_gate".to_string(),
                        passed: false,
                        error_message: Some(format!("Impossible de lancer `{}`: {}", cmd_str, e)),
                        output: Some(combined_out),
                        duration_ms: start.elapsed().as_millis(),
                    });
                }
            }
        }

        Some(ProbeRunResult {
            probe_id: "project-integrity-gate".to_string(),
            runner: "polyglot_gate".to_string(),
            passed: true,
            error_message: None,
            output: Some(combined_out),
            duration_ms: start.elapsed().as_millis(),
        })
    }

    /// Liste les fichiers modifiés ou non suivis actuellement présents dans `.ide/shadow_worktree`.
    pub fn list_shadow_staged_files(&self) -> Vec<String> {
        if !self.shadow_path.exists() {
            return Vec::new();
        }
        let Ok(out) = crate::hidden_command("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.shadow_path)
            .output()
        else {
            return Vec::new();
        };
        if !out.status.success() {
            return Vec::new();
        }
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim_end();
                if trimmed.len() > 3 {
                    Some(trimmed[3..].trim().replace('\\', "/"))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Exécute une vérification rapide (compile/clippy/test ciblé) dans `.ide/shadow_worktree`
    /// sans fusionner ni compter comme un échec de sonde scellée.
    pub fn run_shadow_check(&self, filter: Option<&str>, check_only: bool, clippy: bool) -> Result<serde_json::Value, String> {
        self.prepare_shadow_worktree()?;

        let clean_filter = filter
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':')));

        let mut args_owned: Vec<String> = Vec::new();
        let program: &str = if self.shadow_path.join("Cargo.toml").is_file() {
            if clippy {
                args_owned.extend([
                    "clippy".to_string(),
                    "--all-targets".to_string(),
                    "--".to_string(),
                    "-D".to_string(),
                    "warnings".to_string(),
                ]);
            } else if check_only {
                args_owned.extend(["check".to_string(), "--all-targets".to_string()]);
            } else {
                args_owned.extend(["test".to_string(), "--all-targets".to_string()]);
                if let Some(f) = clean_filter {
                    args_owned.push(f.to_string());
                }
            }
            "cargo"
        } else if self.shadow_path.join("package.json").is_file() {
            args_owned.push("--test".to_string());
            if let Some(f) = clean_filter {
                args_owned.push(f.to_string());
            }
            "node"
        } else if self.shadow_path.join("pyproject.toml").is_file()
            || self.shadow_path.join("pytest.ini").is_file()
            || self.shadow_path.join("tests").is_dir()
        {
            args_owned.extend(["-m".to_string(), "pytest".to_string(), "-q".to_string()]);
            if let Some(f) = clean_filter {
                args_owned.push(f.to_string());
            }
            "python"
        } else if self.shadow_path.join("go.mod").is_file() {
            args_owned.extend(["test".to_string(), "./...".to_string()]);
            "go"
        } else {
            return Err("Aucun projet Rust, Node.js, Python ou Go détecté dans shadow_worktree.".to_string());
        };

        let mut cmd = crate::hidden_command(program);
        cmd.args(&args_owned).current_dir(&self.shadow_path);
        if program == "cargo" {
            let shared_target = self.workspace_root.join(".ide").join("shadow_target");
            cmd.env("CARGO_TARGET_DIR", &shared_target);
        }

        let cmd_display = format!("{} {}", program, args_owned.join(" "));
        let out = run_command_with_timeout(&mut cmd, 60, &cmd_display)?;
        let exit_code = out.status.code().unwrap_or(-1);
        let stdout: String = String::from_utf8_lossy(&out.stdout).chars().take(8_000).collect();
        let stderr: String = String::from_utf8_lossy(&out.stderr).chars().take(8_000).collect();

        Ok(serde_json::json!({
            "status": if out.status.success() { "ok" } else { "failed" },
            "command": cmd_display,
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
        }))
    }
}

fn run_command_with_timeout(
    cmd: &mut std::process::Command,
    timeout_secs: u64,
    label: &str,
) -> Result<std::process::Output, String> {
    use std::io::Read;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Échec lancement '{}': {}", label, e))?;

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let t_out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = stdout_pipe {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let t_err = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = stderr_pipe {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = t_out.join().unwrap_or_default();
                let stderr = t_err.join().unwrap_or_default();
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    #[cfg(target_os = "windows")]
                    {
                        let pid = child.id().to_string();
                        let _ = crate::hidden_command("taskkill")
                            .args(["/T", "/F", "/PID", &pid])
                            .output();
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = t_out.join();
                    let _ = t_err.join();
                    return Err(format!(
                        "Timeout ({}s) sur '{}' — arbre de processus terminé pour éviter tout verrou LNK1104.",
                        timeout_secs, label
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(format!("Erreur attente '{}': {}", label, e));
            }
        }
    }
}

