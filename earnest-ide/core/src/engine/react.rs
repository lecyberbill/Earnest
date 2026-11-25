use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

static CANCEL_RUN: AtomicBool = AtomicBool::new(false);
static THINK_EFFORT: OnceLock<Mutex<String>> = OnceLock::new();
static LIVE_LOGS: OnceLock<Mutex<(u64, VecDeque<LiveLogEntry>)>> = OnceLock::new();

#[derive(Clone, serde::Serialize)]
pub struct LiveLogEntry {
    pub id: u64,
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

pub fn push_live_log(level: &str, message: &str) {
    let lock = LIVE_LOGS.get_or_init(|| Mutex::new((0, VecDeque::with_capacity(512))));
    if let Ok(mut guard) = lock.lock() {
        guard.0 += 1;
        let id = guard.0;
        let ts = chrono::Local::now().format("%H:%M:%S").to_string();
        if guard.1.len() >= 500 {
            guard.1.pop_front();
        }
        guard.1.push_back(LiveLogEntry {
            id,
            timestamp: ts,
            level: level.to_string(),
            message: log_sanitizer::sanitize_text(message),
        });
    }
}

pub fn get_live_logs(since_id: u64) -> Vec<LiveLogEntry> {
    let lock = LIVE_LOGS.get_or_init(|| Mutex::new((0, VecDeque::with_capacity(512))));
    if let Ok(guard) = lock.lock() {
        return guard.1.iter().filter(|e| e.id > since_id).cloned().collect();
    }
    Vec::new()
}

pub fn clear_live_logs() {
    let lock = LIVE_LOGS.get_or_init(|| Mutex::new((0, VecDeque::with_capacity(512))));
    if let Ok(mut guard) = lock.lock() {
        guard.1.clear();
    }
}
use rusqlite::{params, Connection};
use serde_json::Value;

use crate::db::memories::MemoryService;
use crate::llm::client::ChatMessage;
use crate::llm::router::LLMRouter;
use crate::tools::fs::{read_file, write_file, edit_file};
use crate::tools::search::{grep_search, list_files};
use crate::tools::terminal::{execute_terminal, execute_terminal_with_approval};
use crate::tools::web::{search_web, fetch_web_page};
use crate::tools::{get_ide_tools_schemas, get_governance_tools_schemas};
use crate::governance::{
    GovernanceMode, PolicyManager, PolicyConfig,
    InvariantsFile, verify_invariants_lock,
    ShadowRunner
};

pub struct ReActResult {
    pub run_id: String,
    pub status: String,
    pub answer: String,
    pub steps_count: usize,
    pub total_tokens: usize,
    pub duration_ms: u128,
}

pub struct ReActEngine<'a> {
    pub conn: &'a Connection,
    pub router: &'a LLMRouter,
    pub project_id: String,
    pub workspace_root: PathBuf,
    pub max_steps: usize,
}

impl<'a> ReActEngine<'a> {
    pub fn new(conn: &'a Connection, router: &'a LLMRouter, project_id: &str) -> Self {
        // Déterminer la racine du workspace : si project_id correspond à un chemin de dossier existant
        let ws_path = if Path::new(project_id).exists() {
            PathBuf::from(project_id)
        } else {
            // Chercher dans la table projects si configuré
            let found_path: Option<String> = conn.query_row(
                "SELECT root_path FROM projects WHERE id = ?1",
                params![project_id],
                |r| r.get(0),
            ).ok();
            found_path.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
        };

        Self {
            conn,
            router,
            project_id: project_id.to_string(),
            workspace_root: ws_path,
            max_steps: 80,
        }
    }

    pub fn with_workspace(mut self, root: impl AsRef<Path>) -> Self {
        self.workspace_root = root.as_ref().to_path_buf();
        self
    }

    fn record_trace(
        &self,
        run_id: &str,
        step_order: usize,
        step_type: &str,
        tool_name: Option<&str>,
        payload: &str,
    ) {
        self.conn.execute(
            "INSERT INTO run_traces (run_id, step_order, step_type, tool_name, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![run_id, step_order as i64, step_type, tool_name, payload],
        ).ok();
    }

    /// Dispatch des outils classiques & gouvernance
    fn dispatch_tool(
        &self,
        name: &str,
        args: &Value,
        policy: &PolicyConfig,
        failure_counts: &mut HashMap<String, usize>,
    ) -> (String, Option<String>) {
        if let Some(trunc_err) = args.get("__truncated_error").and_then(|v| v.as_str()) {
            return (serde_json::json!({"status": "error", "error": trunc_err}).to_string(), None);
        }
        match name {
            // === OUTILS DE GOUVERNANCE ===

            // 1. write_code (Mode 2 - Sandbox Shadow)
            "write_code" => {
                let target_contract_id = args["target_contract_id"].as_str().or_else(|| args["contract_id"].as_str()).unwrap_or("");
                let raw_target_file = args["target_file"].as_str().unwrap_or("");
                let target_file_owned = match ShadowRunner::normalize_rel_path(&self.workspace_root, raw_target_file) {
                    Ok(rel) => rel,
                    Err(e) => return (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                };
                let target_file = target_file_owned.as_str();
                let patch = args["patch"].as_str().unwrap_or("");

                // Vérification cryptographique stricte du SHA-256 vs invariants.lock
                if let Err(e) = verify_invariants_lock(&self.workspace_root, &policy.invariants_file, &policy.lock_file) {
                    return (serde_json::json!({
                        "status": "INTEGRITY_VIOLATION",
                        "error": format!("Arrêt immédiat : Violation de l'intégrité cryptographique du scellement d'invariants : {}", e)
                    }).to_string(), None);
                }

                // Charger le contrat correspondant
                let inv_path = self.workspace_root.join(&policy.invariants_file);
                let inv_content = match std::fs::read_to_string(&inv_path) {
                    Ok(c) => c,
                    Err(e) => return (serde_json::json!({"status": "error", "error": format!("Lecture invariants.json: {}", e)}).to_string(), None),
                };
                let inv_file: InvariantsFile = match serde_json::from_str(&inv_content) {
                    Ok(f) => f,
                    Err(e) => return (serde_json::json!({"status": "error", "error": format!("Parse invariants.json: {}", e)}).to_string(), None),
                };

                let target_contract = match inv_file.contracts.iter().find(|c| c.id == target_contract_id) {
                    Some(c) => c,
                    None => return (serde_json::json!({
                        "status": "error",
                        "error": format!("Contrat d'invariants '{}' introuvable dans .ide/invariants.json", target_contract_id)
                    }).to_string(), None),
                };

                let resolve_patch = |file_rel: &str, raw_patch: &str, item: &Value| -> Result<String, String> {
                    if !raw_patch.is_empty() {
                        let trimmed = raw_patch.trim();
                        let lower = trimmed.to_lowercase();
                        if trimmed.len() < 120
                            && (lower == "noop"
                                || lower == "no-op"
                                || lower.starts_with("# no-op")
                                || lower.starts_with("// no-op"))
                        {
                            return Err(format!(
                                "Contenu factice '{}' refusé sur '{}' : pour valider sans écraser le fichier, appelez write_code avec stage_only: false et patch: \"\".",
                                trimmed, file_rel
                            ));
                        }
                        if item["append"].as_bool().unwrap_or(false) {
                            let shadow_candidate = self.workspace_root.join(&policy.shadow_env.path).join(file_rel);
                            let abs_path = if shadow_candidate.is_file() {
                                shadow_candidate
                            } else {
                                self.workspace_root.join(file_rel)
                            };
                            let existing = std::fs::read_to_string(&abs_path).unwrap_or_default();
                            let sep = if !existing.is_empty() && !existing.ends_with('\n') && !raw_patch.starts_with('\n') {
                                "\n"
                            } else {
                                ""
                            };
                            return Ok(format!("{}{}{}", existing, sep, raw_patch));
                        }
                        return Ok(raw_patch.to_string());
                    }
                    if let (Some(target), Some(replacement)) = (item["target"].as_str(), item["replacement"].as_str()) {
                        let shadow_candidate = self.workspace_root.join(&policy.shadow_env.path).join(file_rel);
                        let abs_path = if shadow_candidate.is_file() {
                            shadow_candidate
                        } else {
                            self.workspace_root.join(file_rel)
                        };
                        let existing = std::fs::read_to_string(&abs_path)
                            .map_err(|e| format!("Édition ciblée impossible sur '{}': {}", file_rel, e))?;
                        match atomic_patch::apply_patch(&existing, target, replacement) {
                            Ok(patched) => return Ok(patched),
                            Err(m) => match m.status {
                                atomic_patch::MatchStatus::Ambiguous => {
                                    return Err(format!(
                                        "Bloc 'target' ambigu dans '{}': {}",
                                        file_rel,
                                        m.context.unwrap_or_default()
                                    ));
                                }
                                _ => {
                                    return Err(format!(
                                        "Bloc 'target' introuvable dans '{}' pour l'édition ciblée. {}",
                                        file_rel,
                                        m.context.unwrap_or_default()
                                    ));
                                }
                            },
                        }
                    }
                    Ok(String::new())
                };

                // Exécution dans le shadow worktree (mono-fichier ou multi-fichiers atomique via `files`)
                let runner = ShadowRunner::new(&self.workspace_root, &policy.shadow_env.path, policy.shadow_env.timeout_ms);
                if args["stage_only"].as_bool().unwrap_or(false) {
                    if let Err(e) = runner.prepare_shadow_worktree() {
                        return (serde_json::json!({"status": "error", "error": e}).to_string(), None);
                    }
                    let shadow_root = self.workspace_root.join(&policy.shadow_env.path);
                    let mut staged = Vec::new();
                    if let Some(files_arr) = args["files"].as_array().filter(|a| !a.is_empty()) {
                        for item in files_arr {
                            let raw_f = item["target_file"].as_str().or_else(|| item["path"].as_str()).unwrap_or("");
                            let f_owned = match ShadowRunner::normalize_rel_path(&self.workspace_root, raw_f) {
                                Ok(rel) => rel,
                                Err(e) => return (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                            };
                            let f = f_owned.as_str();
                            let p = item["patch"].as_str().or_else(|| item["content"].as_str()).unwrap_or("");
                            if !f.is_empty() {
                                match resolve_patch(f, p, item) {
                                    Ok(resolved) => {
                                        let dest = shadow_root.join(f);
                                        if let Some(parent) = dest.parent() { std::fs::create_dir_all(parent).ok(); }
                                        if let Err(e) = std::fs::write(&dest, resolved) {
                                            return (serde_json::json!({"status": "error", "error": e.to_string()}).to_string(), None);
                                        }
                                        staged.push(f.to_string());
                                    }
                                    Err(e) => return (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                                }
                            }
                        }
                    } else if !target_file.is_empty() {
                        match resolve_patch(target_file, patch, args) {
                            Ok(resolved) => {
                                let dest = shadow_root.join(target_file);
                                if let Some(parent) = dest.parent() { std::fs::create_dir_all(parent).ok(); }
                                if let Err(e) = std::fs::write(&dest, resolved) {
                                    return (serde_json::json!({"status": "error", "error": e.to_string()}).to_string(), None);
                                }
                                staged.push(target_file.to_string());
                            }
                            Err(e) => return (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                        }
                    }
                    return (serde_json::json!({
                        "status": "STAGED",
                        "staged_files": staged,
                        "message": "Fichier(s) écrit(s) dans le shadow worktree sans exécution des sondes. Appelez write_code sans stage_only pour valider et fusionner."
                    }).to_string(), None);
                }
                let exec_result = if let Some(files_arr) = args["files"].as_array().filter(|a| !a.is_empty()) {
                    let mut owned_pairs: Vec<(String, String)> = Vec::with_capacity(files_arr.len());
                    for item in files_arr {
                        let raw_f = item["target_file"].as_str().or_else(|| item["path"].as_str()).unwrap_or("");
                        let f_owned = match ShadowRunner::normalize_rel_path(&self.workspace_root, raw_f) {
                            Ok(rel) => rel,
                            Err(e) => return (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                        };
                        let f = f_owned.as_str();
                        let p = item["patch"].as_str().or_else(|| item["content"].as_str()).unwrap_or("");
                        if !f.is_empty() {
                            match resolve_patch(f, p, item) {
                                Ok(resolved) => owned_pairs.push((f.to_string(), resolved)),
                                Err(e) => return (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                            }
                        }
                    }
                    let pairs: Vec<(&str, &str)> = owned_pairs.iter().map(|(f, p)| (f.as_str(), p.as_str())).collect();
                    let primary = if !target_file.is_empty() {
                        target_file
                    } else {
                        pairs.first().map(|(f, _)| *f).unwrap_or("")
                    };
                    runner.execute_contract_patches(target_contract, &pairs, primary)
                } else if patch.is_empty() && args["target"].as_str().is_none() {
                    // Validation pure de ce qui est déjà stagé dans le shadow worktree
                    runner.execute_contract_patches(target_contract, &[], target_file)
                } else {
                    let resolved_patch = match resolve_patch(target_file, patch, args) {
                        Ok(p) => p,
                        Err(e) => return (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                    };
                    runner.execute_contract_patch(target_contract, target_file, &resolved_patch)
                };

                if exec_result.success {
                    // Réinitialiser les échecs consécutifs pour les sondes de ce contrat
                    for p in &target_contract.probes {
                        failure_counts.remove(&p.probe_id);
                    }
                    self.run_ide_hooks("post_write_code");
                    (serde_json::json!({
                        "status": "SUCCESS",
                        "contract_id": target_contract_id,
                        "merged_to_workspace": true,
                        "message": format!("Patch validé avec succès sur 100% des sondes et fusionné dans le projet réel ({})", target_file),
                        "probes": exec_result.probes
                    }).to_string(), None)
                } else {
                    // Échec d'au moins une sonde : trouver la sonde défaillante et incrémenter son compteur
                    let mut failed_probe_id = String::new();
                    for p in &exec_result.probes {
                        if !p.passed {
                            failed_probe_id = p.probe_id.clone();
                            let count = failure_counts.entry(p.probe_id.clone()).or_insert(0);
                            *count += 1;
                            break;
                        }
                    }

                    let cur_count = if !failed_probe_id.is_empty() {
                        *failure_counts.get(&failed_probe_id).unwrap_or(&1)
                    } else { 1 };

                    if cur_count >= policy.guardrails.max_consecutive_failures {
                        let alert_msg = format!(
                            "SEUIL CRITIQUE ATTEINT : {} échecs consécutifs sur la sonde '{}'. L'outil write_code est maintenant BLOQUÉ. Vous DEVEZ obligatoirement appeler 'report_checkpoint_status' pour fournir un diagnostic de cause racine et une stratégie avant toute reprise.",
                            cur_count, failed_probe_id
                        );
                        (serde_json::json!({
                            "status": "FAILURE_THRESHOLD_EXCEEDED",
                            "consecutive_failures": cur_count,
                            "failed_probe_id": failed_probe_id,
                            "failure_reason": exec_result.failure_reason,
                            "probes": exec_result.probes,
                            "directive": alert_msg
                        }).to_string(), None)
                    } else {
                        (serde_json::json!({
                            "status": "PROBE_FAILED",
                            "consecutive_failures": cur_count,
                            "failed_probe_id": failed_probe_id,
                            "failure_reason": exec_result.failure_reason,
                            "probes": exec_result.probes,
                            "message": "Le patch a échoué en sandbox shadow worktree. L'espace de travail réel est resté 100% intact. Corrigez le code."
                        }).to_string(), None)
                    }
                }
            }

            // 2a. add_invariant (Formulaire plat — Mode 3 Architecture)
            "add_invariant" => {
                let inv_path = self.workspace_root.join(&policy.invariants_file);
                if let Some(parent) = inv_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                let mut inv_val: serde_json::Value = std::fs::read_to_string(&inv_path)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_else(|| serde_json::json!({
                        "schema_version": "1.0",
                        "project_id": self.workspace_root.file_name().and_then(|n| n.to_str()).unwrap_or("project"),
                        "contracts": []
                    }));

                let existing_len = inv_val["contracts"].as_array().map(|a| a.len()).unwrap_or(0);
                let cid = args["id"].as_str().filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("C{}", existing_len + 1));
                let title = args["title"].as_str().unwrap_or(&cid).to_string();
                let statement = args["statement"].as_str().or_else(|| args["description"].as_str()).unwrap_or(&title).to_string();
                let target_file = args["target_file"].as_str().unwrap_or("src/lib.rs").to_string();
                let test_query = args["test_query"].as_str().or_else(|| args["query"].as_str()).unwrap_or(&title).to_string();
                let runner = args["runner"].as_str().filter(|r| *r == "cargo_test" || *r == "static").unwrap_or("cargo_test").to_string();
                let criticality = args["criticality"].as_str().unwrap_or("high").to_string();

                let contract = serde_json::json!({
                    "id": cid,
                    "title": title,
                    "statement": statement,
                    "criticality": criticality,
                    "target_file": target_file,
                    "probes": [{
                        "probe_id": format!("{}-P1", cid),
                        "runner": runner,
                        "query": test_query,
                        "command": serde_json::Value::Null,
                        "expect": "pass"
                    }]
                });
                if let Some(arr) = inv_val.get_mut("contracts").and_then(|c| c.as_array_mut()) {
                    if let Some(pos) = arr.iter().position(|c| c["id"].as_str() == Some(cid.as_str())) {
                        arr[pos] = contract;
                    } else {
                        arr.push(contract);
                    }
                }
                let total = inv_val["contracts"].as_array().map(|a| a.len()).unwrap_or(0);
                let json_str = serde_json::to_string_pretty(&inv_val).unwrap_or_default();
                crate::governance::invariants::set_file_readonly(&inv_path, false).ok();
                if let Err(e) = std::fs::write(&inv_path, &json_str) {
                    return (serde_json::json!({
                        "status": "error",
                        "error": format!("Écriture .ide/invariants.json échouée: {}", e)
                    }).to_string(), None);
                }
                let sha256 = crate::governance::invariants::sha256_hex(json_str.as_bytes());
                return (serde_json::json!({
                    "status": "ok",
                    "contract_id": cid,
                    "total_contracts": total,
                    "sha256": sha256,
                    "message": "Contract persisted in .ide/invariants.json. Call request_architecture_review when all contracts are ready."
                }).to_string(), None);
            }

            // 2. submit_invariants (Mode 3 Architecture)
            "submit_invariants" => {
                let mut invariants_val = args["invariants"].clone();
                if let Some(obj) = invariants_val.as_object_mut() {
                    if !obj.contains_key("schema_version") {
                        obj.insert("schema_version".into(), serde_json::json!("1.0"));
                    }
                    if let Some(contracts) = obj.get_mut("contracts").and_then(|c| c.as_array_mut()) {
                        for (c_idx, c) in contracts.iter_mut().enumerate() {
                            if let Some(c_obj) = c.as_object_mut() {
                                let cid = c_obj.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_else(|| format!("C{}", c_idx + 1));
                                if !c_obj.contains_key("title") {
                                    if let Some(name) = c_obj.get("name").cloned() {
                                        c_obj.insert("title".into(), name);
                                    } else {
                                        c_obj.insert("title".into(), serde_json::json!(cid));
                                    }
                                }
                                if !c_obj.contains_key("statement") {
                                    if let Some(desc) = c_obj.get("description").cloned() {
                                        c_obj.insert("statement".into(), desc);
                                    }
                                }
                                if !c_obj.contains_key("probes") {
                                    if let Some(probs) = c_obj.remove("probs") {
                                        c_obj.insert("probes".into(), probs);
                                    }
                                }
                                let default_target = c_obj.get("target_file").and_then(|v| v.as_str()).unwrap_or("src/lib.rs").to_string();
                                if let Some(syms) = c_obj.get_mut("symbols").and_then(|s| s.as_array_mut()) {
                                    for sym in syms.iter_mut() {
                                        if let Some(s) = sym.as_str().map(|s| s.to_string()) {
                                            *sym = serde_json::json!({
                                                "name": s,
                                                "kind": "function",
                                                "file": default_target
                                            });
                                        }
                                    }
                                }
                                if let Some(probes) = c_obj.get_mut("probes").and_then(|p| p.as_array_mut()) {
                                    for (p_idx, p) in probes.iter_mut().enumerate() {
                                        if let Some(s) = p.as_str().map(|s| s.to_string()) {
                                            let is_static = s.starts_with("static");
                                            let q = s.trim_start_matches("cargo test ").trim_start_matches("test::").trim().to_string();
                                            *p = serde_json::json!({
                                                "probe_id": format!("{}-P{}", cid, p_idx + 1),
                                                "kind": "unit",
                                                "target_file": default_target,
                                                "test_query": q,
                                                "runner": if is_static { "static" } else { "cargo_test" },
                                                "expected": "pass"
                                            });
                                        }
                                        if let Some(p_obj) = p.as_object_mut() {
                                            if !p_obj.contains_key("probe_id") {
                                                let pid = p_obj.get("id").and_then(|v| v.as_str()).map(|s| s.to_string())
                                                    .unwrap_or_else(|| format!("{}-P{}", cid, p_idx + 1));
                                                p_obj.insert("probe_id".into(), serde_json::json!(pid));
                                            }
                                            if !p_obj.contains_key("test_query") {
                                                if let Some(q) = p_obj.get("query").cloned() {
                                                    p_obj.insert("test_query".into(), q);
                                                }
                                            }
                                            if !p_obj.contains_key("target_file") {
                                                p_obj.insert("target_file".into(), serde_json::json!(default_target));
                                            }
                                            if !p_obj.contains_key("runner") {
                                                let kind = p_obj.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                                                let r = if kind.contains("static") { "static" } else { "cargo_test" };
                                                p_obj.insert("runner".into(), serde_json::json!(r));
                                            } else if let Some(r) = p_obj.get("runner").and_then(|v| v.as_str()).map(|s| s.to_string()) {
                                                if r.contains('|') {
                                                    let fixed = if r.contains("cargo_test") { "cargo_test" } else { "static" };
                                                    p_obj.insert("runner".into(), serde_json::json!(fixed));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let invariants_val = &invariants_val;
                if let Err(e) = serde_json::from_value::<InvariantsFile>(invariants_val.clone()) {
                    return (serde_json::json!({
                        "status": "error",
                        "error": format!("Schéma InvariantsFile invalide: {}.", e),
                        "expected_schema": {
                            "project_id": "string",
                            "contracts": [{
                                "id": "C1",
                                "title": "string",
                                "statement": "string",
                                "criticality": "critical|high|medium",
                                "target_file": "path/to/file.rs",
                                "probes": [{
                                    "probe_id": "C1-P1",
                                    "runner": "cargo_test|cli|static",
                                    "query": "test_name_or_regex_or_null",
                                    "command": "cli_cmd_or_null",
                                    "expect": "pass"
                                }]
                            }]
                        }
                    }).to_string(), None);
                }
                let inv_path = self.workspace_root.join(&policy.invariants_file);
                if let Some(parent) = inv_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }

                // Déverrouiller temporairement si existant
                crate::governance::invariants::set_file_readonly(&inv_path, false).ok();
                match serde_json::to_string_pretty(invariants_val) {
                    Ok(json_str) => match std::fs::write(&inv_path, &json_str) {
                        Ok(_) => {
                            let hash = crate::governance::invariants::sha256_hex(json_str.as_bytes());
                            (serde_json::json!({
                                "status": "submitted",
                                "file": policy.invariants_file,
                                "sha256": hash,
                                "message": "Invariants enregistrés dans .ide/invariants.json. Vous devez maintenant appeler 'request_architecture_review' pour soumettre à l'approbation humaine."
                            }).to_string(), None)
                        }
                        Err(e) => (serde_json::json!({"status": "error", "error": format!("Écriture invariants: {}", e)}).to_string(), None),
                    },
                    Err(e) => (serde_json::json!({"status": "error", "error": format!("Sérialisation invariants: {}", e)}).to_string(), None),
                }
            }

            // 3. request_architecture_review (Interruption asynchrone pour revue humaine)
            "request_architecture_review" => {
                let summary = args["summary"].as_str().unwrap_or("");
                let inv_path = self.workspace_root.join(&policy.invariants_file);
                if !inv_path.is_file() {
                    return (serde_json::json!({
                        "status": "error",
                        "error": "Aucun fichier .ide/invariants.json trouvé : appelle d'abord l'outil `add_invariant` (vrai appel d'outil JSON, pas du texte libre) avant `request_architecture_review`."
                    }).to_string(), None);
                }
                let computed_hash = std::fs::read(&inv_path)
                    .ok()
                    .map(|bytes| crate::governance::invariants::sha256_hex(&bytes))
                    .unwrap_or_default();
                let hash = if !computed_hash.is_empty() {
                    computed_hash
                } else {
                    args["invariants_hash"].as_str().unwrap_or("").to_string()
                };
                (serde_json::json!({
                    "status": "awaiting_approval",
                    "action": "architecture_review",
                    "summary": summary,
                    "invariants_hash": hash,
                    "message": "Revue d'architecture demandée. En attente de confirmation humaine dans l'UI Earnest IDE..."
                }).to_string(), Some("AwaitingArchitectureReview".to_string()))
            }

            // 4. query_invariants (Mode 2)
            "query_invariants" => {
                let target = args["target_file_or_symbol"].as_str().or_else(|| args["contract_id"].as_str()).unwrap_or("");
                let inv_path = self.workspace_root.join(&policy.invariants_file);
                if !inv_path.exists() {
                    return (serde_json::json!({"status": "not_found", "message": "Aucun fichier invariants.json trouvé"}).to_string(), None);
                }
                match std::fs::read_to_string(&inv_path) {
                    Ok(content) => {
                        let inv_file: Result<InvariantsFile, _> = serde_json::from_str(&content);
                        match inv_file {
                            Ok(f) => {
                                if target.is_empty() {
                                    (serde_json::json!({
                                        "status": "success",
                                        "project_id": f.project_id,
                                        "contracts": f.contracts
                                    }).to_string(), None)
                                } else {
                                    let matched: Vec<_> = f.contracts.into_iter().filter(|c| {
                                        c.id == target || c.target_file == target || c.symbols.iter().any(|s| s.name == target)
                                    }).collect();
                                    (serde_json::json!({
                                        "status": "success",
                                        "query": target,
                                        "matched_contracts": matched
                                    }).to_string(), None)
                                }
                            }
                            Err(e) => (serde_json::json!({"status": "error", "error": format!("Parse: {}", e)}).to_string(), None),
                        }
                    }
                    Err(e) => (serde_json::json!({"status": "error", "error": format!("Lecture: {}", e)}).to_string(), None),
                }
            }

            // 5. request_contract_mutation (Interruption asynchrone pour mutation humaine)
            "request_contract_mutation" => {
                let contract_id = args["target_contract_id"].as_str().or_else(|| args["contract_id"].as_str()).unwrap_or("");
                let reason = args["reason"].as_str().or_else(|| args["justification"].as_str()).unwrap_or("");
                let diff = args["diff"].as_str().unwrap_or("");
                (serde_json::json!({
                    "status": "awaiting_approval",
                    "action": "contract_mutation",
                    "target_contract_id": contract_id,
                    "reason": reason,
                    "diff": diff,
                    "message": "Demande de mutation de contrat soumise. En attente de confirmation humaine dans l'UI..."
                }).to_string(), Some("AwaitingContractMutation".to_string()))
            }

            // 6. report_checkpoint_status (Interruption pour checkpoint réflexif)
            "report_checkpoint_status" => {
                let probe_id = args["failed_probe_id"].as_str().unwrap_or("");
                let autopsy = args["root_cause_analysis"].as_str().or_else(|| args["error_autopsy"].as_str()).unwrap_or("");
                let next_strat = args["next_attempt_strategy"].as_str().or_else(|| args["countermeasure"].as_str()).unwrap_or("");
                let req_type = args["request_type"].as_str().unwrap_or("authorize_retry_4");

                (serde_json::json!({
                    "status": "awaiting_approval",
                    "action": "checkpoint_status",
                    "failed_probe_id": probe_id,
                    "root_cause_analysis": autopsy,
                    "next_attempt_strategy": next_strat,
                    "request_type": req_type,
                    "message": "Rapport réflexif d'autopsie soumis. En attente d'autorisation de reprise dans l'UI..."
                }).to_string(), Some("AwaitingCheckpointAuthorization".to_string()))
            }

            // === OUTILS STANDARDS ===
            "ask_user_question" => {
                let question = args["question"].as_str().unwrap_or("").trim();
                if question.is_empty() {
                    return (
                        serde_json::json!({"status": "error", "error": "Paramètre 'question' vide"}).to_string(),
                        None,
                    );
                }
                let mut md = format!("❓ **Clarification souhaitée**\n\n{}", question);
                if let Some(opts) = args["options"].as_array() {
                    let valid_opts: Vec<&str> = opts
                        .iter()
                        .filter_map(|v| v.as_str().map(str::trim))
                        .filter(|s| !s.is_empty())
                        .collect();
                    if !valid_opts.is_empty() {
                        md.push_str("\n\n");
                        for (idx, opt) in valid_opts.iter().enumerate() {
                            md.push_str(&format!("{}. {}\n", idx + 1, opt));
                        }
                    }
                }
                (md.trim_end().to_string(), Some("user_question".to_string()))
            }
            "web_search" => {
                let query = args["query"].as_str().unwrap_or("");
                let max = args["max_results"].as_u64().unwrap_or(5) as usize;
                match search_web(query, max) {
                    Ok(res) => (res.to_string(), None),
                    Err(e) => (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                }
            }
            "web_fetch" => {
                let url = args["url"].as_str().unwrap_or("");
                let max = args["max_chars"].as_u64().unwrap_or(6000) as usize;
                match fetch_web_page(url, max) {
                    Ok(res) => (res.to_string(), None),
                    Err(e) => (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                }
            }
            "shadow_check" => {
                let filter = args["filter"].as_str();
                let check_only = args["check_only"].as_bool().unwrap_or(false);
                let clippy = args["clippy"].as_bool().unwrap_or(false);
                let runner = ShadowRunner::new(&self.workspace_root, &policy.shadow_env.path, policy.shadow_env.timeout_ms);
                match runner.run_shadow_check(filter, check_only, clippy) {
                    Ok(res) => (res.to_string(), None),
                    Err(e) => (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                }
            }
            "shadow_copy" => {
                let raw_from = args["from_path"].as_str().unwrap_or("");
                let raw_to = args["to_path"].as_str().unwrap_or("");
                let from_rel = match ShadowRunner::normalize_rel_path(&self.workspace_root, raw_from) {
                    Ok(r) => r,
                    Err(e) => return (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                };
                let to_rel = match ShadowRunner::normalize_rel_path(&self.workspace_root, raw_to) {
                    Ok(r) => r,
                    Err(e) => return (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                };
                let runner = ShadowRunner::new(&self.workspace_root, &policy.shadow_env.path, policy.shadow_env.timeout_ms);
                if let Err(e) = runner.prepare_shadow_worktree() {
                    return (serde_json::json!({"status": "error", "error": e}).to_string(), None);
                }
                let shadow_root = self.workspace_root.join(&policy.shadow_env.path);
                let src_shadow = shadow_root.join(&from_rel);
                let src_path = if src_shadow.is_file() { src_shadow } else { self.workspace_root.join(&from_rel) };
                if !src_path.is_file() {
                    return (serde_json::json!({"status": "error", "error": format!("Fichier source introuvable: {}", from_rel)}).to_string(), None);
                }
                let dst_path = shadow_root.join(&to_rel);
                if let Some(parent) = dst_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                match std::fs::copy(&src_path, &dst_path) {
                    Ok(bytes) => (serde_json::json!({
                        "status": "STAGED",
                        "from": from_rel,
                        "to": to_rel,
                        "bytes": bytes,
                        "message": "Fichier copié dans shadow_worktree sans consommer de tokens. Vous pouvez l'éditer avec write_code (target/replacement) ou le valider."
                    }).to_string(), None),
                    Err(e) => (serde_json::json!({"status": "error", "error": e.to_string()}).to_string(), None),
                }
            }
            "shadow_validate_and_merge" => {
                let target_contract_id = args["target_contract_id"].as_str().or_else(|| args["contract_id"].as_str()).unwrap_or("");
                if let Err(e) = verify_invariants_lock(&self.workspace_root, &policy.invariants_file, &policy.lock_file) {
                    return (serde_json::json!({
                        "status": "INTEGRITY_VIOLATION",
                        "error": format!("Arrêt immédiat : Violation de l'intégrité cryptographique du scellement d'invariants : {}", e)
                    }).to_string(), None);
                }
                let inv_path = self.workspace_root.join(&policy.invariants_file);
                let inv_content = match std::fs::read_to_string(&inv_path) {
                    Ok(c) => c,
                    Err(e) => return (serde_json::json!({"status": "error", "error": format!("Lecture invariants.json: {}", e)}).to_string(), None),
                };
                let inv_file: InvariantsFile = match serde_json::from_str(&inv_content) {
                    Ok(f) => f,
                    Err(e) => return (serde_json::json!({"status": "error", "error": format!("Parse invariants.json: {}", e)}).to_string(), None),
                };
                let target_contract = match inv_file.contracts.iter().find(|c| c.id == target_contract_id) {
                    Some(c) => c,
                    None => return (serde_json::json!({
                        "status": "error",
                        "error": format!("Contrat d'invariants '{}' introuvable dans .ide/invariants.json", target_contract_id)
                    }).to_string(), None),
                };
                let raw_target_file = args["target_file"].as_str().unwrap_or(&target_contract.target_file);
                let target_file = match ShadowRunner::normalize_rel_path(&self.workspace_root, raw_target_file) {
                    Ok(rel) => rel,
                    Err(e) => return (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                };
                let runner = ShadowRunner::new(&self.workspace_root, &policy.shadow_env.path, policy.shadow_env.timeout_ms);
                let exec_result = runner.execute_contract_patches(target_contract, &[], &target_file);
                if exec_result.success {
                    for p in &target_contract.probes {
                        failure_counts.remove(&p.probe_id);
                    }
                    self.run_ide_hooks("post_write_code");
                    (serde_json::json!({
                        "status": "SUCCESS",
                        "contract_id": target_contract_id,
                        "merged_to_workspace": true,
                        "message": "Fichiers shadow validés à 100% et matérialisés dans le projet réel sans coût token.",
                        "probes": exec_result.probes
                    }).to_string(), None)
                } else {
                    let mut failed_probe_id = String::new();
                    for p in &exec_result.probes {
                        if !p.passed {
                            failed_probe_id = p.probe_id.clone();
                            let count = failure_counts.entry(p.probe_id.clone()).or_insert(0);
                            *count += 1;
                            break;
                        }
                    }
                    let cur_count = if !failed_probe_id.is_empty() { *failure_counts.get(&failed_probe_id).unwrap_or(&1) } else { 1 };
                    (serde_json::json!({
                        "status": if cur_count >= policy.guardrails.max_consecutive_failures { "FAILURE_THRESHOLD_EXCEEDED" } else { "PROBE_FAILED" },
                        "consecutive_failures": cur_count,
                        "failed_probe_id": failed_probe_id,
                        "failure_reason": exec_result.failure_reason,
                        "probes": exec_result.probes,
                        "message": "Validation shadow échouée. Utilisez `shadow_check` pour diagnostiquer sans pénalité avant de relancer `shadow_validate_and_merge`."
                    }).to_string(), None)
                }
            }
            "read_file" | "grep_search" | "list_files" => {
                (Self::exec_readonly_tool(&self.workspace_root, name, args, policy), None)
            }
            "edit_file" => {
                if policy.mode == GovernanceMode::Architecture {
                    return (serde_json::json!({
                        "status": "error",
                        "error": "Outil 'edit_file' strictement interdit en Mode 3 Architecture. Vous devez uniquement formaliser les invariants avec 'submit_invariants' et 'request_architecture_review'."
                    }).to_string(), None);
                }
                let path = args["path"].as_str().unwrap_or("");
                let resolved = if Path::new(path).is_absolute() {
                    PathBuf::from(path)
                } else {
                    self.workspace_root.join(path)
                };
                let target = args["target"].as_str().unwrap_or("");
                let replacement = args["replacement"].as_str().unwrap_or("");
                match edit_file(resolved.to_str().unwrap_or(path), target, replacement) {
                    Ok(res) => (res.to_string(), None),
                    Err(e) => (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                }
            }
            "write_file" => {
                if policy.mode == GovernanceMode::Architecture {
                    return (serde_json::json!({
                        "status": "error",
                        "error": "Outil 'write_file' strictement interdit en Mode 3 Architecture. Zéro écriture de code autorisée avant scellement formel des invariants."
                    }).to_string(), None);
                }
                let path = args["path"].as_str().unwrap_or("");
                let resolved = if Path::new(path).is_absolute() {
                    PathBuf::from(path)
                } else {
                    self.workspace_root.join(path)
                };
                let content = args["content"].as_str().unwrap_or("");
                let final_content = if args["append"].as_bool().unwrap_or(false) {
                    let existing = std::fs::read_to_string(&resolved).unwrap_or_default();
                    let sep = if !existing.is_empty() && !existing.ends_with('\n') && !content.starts_with('\n') {
                        "\n"
                    } else {
                        ""
                    };
                    format!("{}{}{}", existing, sep, content)
                } else {
                    content.to_string()
                };
                match write_file(resolved.to_str().unwrap_or(path), &final_content) {
                    Ok(res) => (res.to_string(), None),
                    Err(e) => (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                }
            }
            "terminal_exec" => {
                if policy.mode == GovernanceMode::Architecture {
                    return (serde_json::json!({
                        "status": "error",
                        "error": "Outil 'terminal_exec' interdit en Mode 3 Architecture."
                    }).to_string(), None);
                }
                let cmd = args["command"].as_str().unwrap_or("");
                let timeout = args["timeout_sec"].as_u64().unwrap_or(60);
                let (res, interrupt) = execute_terminal_with_approval(cmd, timeout, Some(&self.workspace_root), true);
                match res {
                    Ok(val) => (val.to_string(), interrupt),
                    Err(e) => (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                }
            }
            "store_memory" => {
                let key = args["key"].as_str().unwrap_or("");
                let content = args["content"].as_str().unwrap_or("");
                let scope = args["scope"].as_str().unwrap_or("project");
                let mem = MemoryService::new(self.conn);
                match mem.store_memory(&self.project_id, key, content, scope, None) {
                    Ok(msg) => (serde_json::json!({"status": "success", "message": msg}).to_string(), None),
                    Err(e) => (serde_json::json!({"status": "error", "error": e.to_string()}).to_string(), None),
                }
            }
            "recall_memory" => {
                let key = args["key"].as_str().unwrap_or("");
                let mem = MemoryService::new(self.conn);
                match mem.recall_memory(&self.project_id, key, None) {
                    Ok(Some(val)) => (serde_json::json!({"status": "success", "key": key, "content": val}).to_string(), None),
                    Ok(None) => (serde_json::json!({"status": "not_found", "message": format!("Aucune mémoire pour '{}'", key)}).to_string(), None),
                    Err(e) => (serde_json::json!({"status": "error", "error": e.to_string()}).to_string(), None),
                }
            }
            "recall_recent_runs" => {
                let query = args["query"].as_str().unwrap_or("").trim();
                let limit = args["limit"].as_u64().unwrap_or(5).clamp(1, 20) as i64;
                let mut results = Vec::new();
                if query.is_empty() {
                    if let Ok(mut stmt) = self.conn.prepare(
                        "SELECT id, status, prompt, COALESCE(final_answer, ''), created_at FROM runs WHERE project_id = ?1 ORDER BY rowid DESC LIMIT ?2"
                    ) {
                        if let Ok(rows) = stmt.query_map(params![self.project_id, limit], |r| {
                            Ok(serde_json::json!({
                                "run_id": r.get::<_, String>(0)?,
                                "status": r.get::<_, String>(1)?,
                                "prompt": context_trimmer::truncate_tool_output(&r.get::<_, String>(2)?, 300),
                                "final_answer": context_trimmer::truncate_tool_output(&r.get::<_, String>(3)?, 500),
                                "created_at": r.get::<_, String>(4)?,
                            }))
                        }) {
                            for item in rows.flatten() { results.push(item); }
                        }
                    }
                } else {
                    let pattern = format!("%{}%", query);
                    if let Ok(mut stmt) = self.conn.prepare(
                        "SELECT DISTINCT r.id, r.status, r.prompt, COALESCE(r.final_answer, ''), r.created_at FROM runs r LEFT JOIN run_traces t ON t.run_id = r.id WHERE r.project_id = ?1 AND (r.prompt LIKE ?2 OR r.final_answer LIKE ?2 OR t.payload LIKE ?2) ORDER BY r.rowid DESC LIMIT ?3"
                    ) {
                        if let Ok(rows) = stmt.query_map(params![self.project_id, pattern, limit], |r| {
                            Ok(serde_json::json!({
                                "run_id": r.get::<_, String>(0)?,
                                "status": r.get::<_, String>(1)?,
                                "prompt": context_trimmer::truncate_tool_output(&r.get::<_, String>(2)?, 300),
                                "final_answer": context_trimmer::truncate_tool_output(&r.get::<_, String>(3)?, 500),
                                "created_at": r.get::<_, String>(4)?,
                            }))
                        }) {
                            for item in rows.flatten() { results.push(item); }
                        }
                    }
                }
                (serde_json::json!({"status": "success", "count": results.len(), "runs": results}).to_string(), None)
            }
            "invoke_subagent" => {
                let role = args["role"].as_str().unwrap_or("Specialized Subagent");
                let task = args["task"].as_str().unwrap_or("");
                let readonly = args["readonly"].as_bool().unwrap_or(false);

                let sub_engine = SubAgentRunner {
                    conn: self.conn,
                    router: self.router,
                    project_id: format!("{}/sub_{}", self.project_id, &uuid::Uuid::new_v4().to_string().replace('-', "")[..6]),
                    role: role.to_string(),
                    readonly,
                    max_steps: 5,
                };

                match sub_engine.run(task) {
                    Ok(report) => (serde_json::json!({
                        "status": "success",
                        "subagent_role": role,
                        "report": report
                    }).to_string(), None),
                    Err(e) => (serde_json::json!({
                        "status": "error",
                        "subagent_role": role,
                        "error": e
                    }).to_string(), None),
                }
            }
            "kb_list_products" => {
                let mcp = kb_mcp_client();
                match mcp.call_tool("kb_list_products", &serde_json::json!({})) {
                    Ok(res) => (res.to_string(), None),
                    Err(e) => (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                }
            }
            "kb_search" => {
                let pid = args["product_id"].as_str().unwrap_or("earnest");
                let q = args["query"].as_str().unwrap_or("");
                let mcp = kb_mcp_client();
                let call_args = serde_json::json!({ "product_id": pid, "query": q });
                match mcp.call_tool("kb_search", &call_args) {
                    Ok(res) => (res.to_string(), None),
                    Err(e) => (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                }
            }
            "kb_propose" => {
                let pid = args["product_id"].as_str().unwrap_or("earnest");
                let subject_key = args["subject_key"].as_str().unwrap_or("");
                let kind = args["kind"].as_str().unwrap_or("piege");
                let title = args["title"].as_str().unwrap_or("");
                let symptom = args["symptom"].as_str();
                let cause = args["cause"].as_str();
                let correction = args["correction"].as_str().unwrap_or("");
                let example = args["example"].as_str();

                let db_path = crate::tools::resolve_kb_db_path();
                if let Ok(conn) = rusqlite::Connection::open(&db_path) {
                    let _ = conn.execute(
                        "INSERT OR IGNORE INTO products (product_id, name, current_version) VALUES (?, ?, '1.0.0')",
                        rusqlite::params![pid, pid],
                    );
                }

                let mcp = kb_mcp_client();
                mcp.call_tool("kb_search", &serde_json::json!({ "product_id": pid, "query": subject_key })).ok();

                let mut propose_args = serde_json::json!({
                    "product_id": pid,
                    "subject_key": subject_key,
                    "kind": kind,
                    "source": "agent_discovery",
                    "title": title,
                    "correction": correction
                });

                if let Some(s) = symptom { propose_args["symptom"] = serde_json::json!(s); }
                if let Some(c) = cause { propose_args["cause"] = serde_json::json!(c); }
                if let Some(e) = example { propose_args["example"] = serde_json::json!(e); }

                match mcp.call_tool("kb_propose", &propose_args) {
                    Ok(res) => (res.to_string(), None),
                    Err(e) => (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                }
            }
            "symbol_search" => {
                let query = args["query"].as_str().unwrap_or("");
                let sub_path = args["path"].as_str().unwrap_or("");
                let limit = args["limit"].as_u64().unwrap_or(8).clamp(1, 25) as usize;
                let include_snippet = args["include_snippet"].as_bool().unwrap_or(true);
                let root = if sub_path.is_empty() || sub_path == "." {
                    self.workspace_root.clone()
                } else {
                    self.workspace_root.join(sub_path)
                };
                match symbol_index::SymbolIndex::build(&root) {
                    Ok(idx) => {
                        let hits: Vec<Value> = idx
                            .search(query, limit)
                            .into_iter()
                            .map(|entry| {
                                let snippet = if include_snippet {
                                    let full = root.join(&entry.file);
                                    symbol_index::read_symbol_snippet(&full, entry.line, 2, 8)
                                        .ok()
                                        .map(|s| s.content)
                                } else {
                                    None
                                };
                                serde_json::json!({
                                    "name": entry.name,
                                    "kind": format!("{:?}", entry.kind),
                                    "file": entry.file.to_string_lossy(),
                                    "line": entry.line,
                                    "signature": entry.signature,
                                    "snippet": snippet,
                                })
                            })
                            .collect();
                        (
                            serde_json::json!({
                                "status": "ok",
                                "total_indexed": idx.len(),
                                "matches": hits,
                            })
                            .to_string(),
                            None,
                        )
                    }
                    Err(e) => (
                        serde_json::json!({"status": "error", "error": e.to_string()}).to_string(),
                        None,
                    ),
                }
            }
            "workspace_map" => {
                let sub_path = args["path"].as_str().unwrap_or("");
                let max_files = args["max_files"].as_u64().unwrap_or(25).clamp(1, 100) as usize;
                let max_symbols = args["max_symbols_per_file"].as_u64().unwrap_or(8).clamp(1, 30) as usize;
                let root = if sub_path.is_empty() || sub_path == "." {
                    self.workspace_root.clone()
                } else {
                    self.workspace_root.join(sub_path)
                };
                match symbol_index::workspace_map(&root, max_files, max_symbols) {
                    Ok(map) => (
                        serde_json::json!({
                            "status": "ok",
                            "map": map,
                        })
                        .to_string(),
                        None,
                    ),
                    Err(e) => (
                        serde_json::json!({"status": "error", "error": e.to_string()}).to_string(),
                        None,
                    ),
                }
            }
            _ => (serde_json::json!({"status": "error", "error": format!("Outil inconnu : {}", name)}).to_string(), None),
        }
    }

    fn exec_readonly_tool(workspace_root: &Path, name: &str, args: &Value, policy: &PolicyConfig) -> String {
        match name {
            "read_file" => {
                let path = args["path"]
                    .as_str()
                    .or_else(|| args["file_path"].as_str())
                    .or_else(|| args["target_file"].as_str())
                    .unwrap_or("")
                    .trim();
                if path.is_empty() {
                    let err = tool_guard::error::GuardError::EmptyPath;
                    return serde_json::json!({"status": "error", "error": err.to_string()}).to_string();
                }
                let raw_p = Path::new(path);
                if tool_guard::path::has_traversal(raw_p) {
                    let err = tool_guard::error::GuardError::PathTraversal {
                        requested: path.to_string(),
                        allowed_root: workspace_root.display().to_string(),
                    };
                    return serde_json::json!({"status": "error", "error": err.to_string()}).to_string();
                }
                if tool_guard::path::is_system_sensitive(raw_p) {
                    let err = tool_guard::error::GuardError::SystemPath { requested: path.to_string() };
                    return serde_json::json!({"status": "error", "error": err.to_string()}).to_string();
                }
                let rel_for_shadow = if raw_p.is_absolute() {
                    raw_p.strip_prefix(workspace_root).ok()
                } else {
                    Some(raw_p)
                };
                let resolved = if let Some(rel) = rel_for_shadow {
                    let shadow_candidate = workspace_root.join(&policy.shadow_env.path).join(rel);
                    if policy.mode == GovernanceMode::Constrained && shadow_candidate.is_file() {
                        shadow_candidate
                    } else if raw_p.is_absolute() {
                        PathBuf::from(path)
                    } else {
                        workspace_root.join(rel)
                    }
                } else {
                    PathBuf::from(path)
                };
                if let Ok(meta) = std::fs::metadata(&resolved) {
                    if let Err(err) = tool_guard::file::enforce_size_bound(meta.len()) {
                        return serde_json::json!({"status": "error", "error": err.to_string()}).to_string();
                    }
                    if meta.is_file() {
                        if let Ok(mut f) = std::fs::File::open(&resolved) {
                            use std::io::Read;
                            let mut buf = [0u8; 4096];
                            if let Ok(n) = f.read(&mut buf) {
                                if tool_guard::file::is_binary(&buf[..n]) {
                                    let err = tool_guard::error::GuardError::BinaryFile { path: path.to_string() };
                                    return serde_json::json!({"status": "error", "error": err.to_string()}).to_string();
                                }
                            }
                        }
                    }
                }
                let start_line = args["start_line"].as_u64().map(|v| v as usize);
                let end_line = args["end_line"].as_u64().map(|v| v as usize);
                match read_file(resolved.to_str().unwrap_or(path), start_line, end_line) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "grep_search" => {
                let query = args["query"].as_str().unwrap_or("");
                let raw_root = args["root_path"].as_str().unwrap_or(".");
                let resolved_root = if Path::new(raw_root).is_absolute() {
                    PathBuf::from(raw_root)
                } else {
                    workspace_root.join(raw_root)
                };
                let root = resolved_root.to_str().unwrap_or(raw_root);
                let max = args["max_results"].as_u64().map(|v| v as usize);
                match grep_search(root, query, max) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "list_files" => {
                let raw_root = args["root_path"].as_str().unwrap_or(".");
                let resolved_root = if Path::new(raw_root).is_absolute() {
                    PathBuf::from(raw_root)
                } else {
                    workspace_root.join(raw_root)
                };
                let root = resolved_root.to_str().unwrap_or(raw_root);
                let depth = args["max_depth"].as_u64().map(|v| v as usize);
                match list_files(root, depth) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            _ => serde_json::json!({"status": "error", "error": format!("Outil inconnu : {}", name)}).to_string(),
        }
    }

    /// Filtrage strict des outils exposés au LLM selon la Machine à États (Section 2 du CDC)
    pub fn get_tools_for_policy(&self, policy: &PolicyConfig) -> Vec<Value> {
        let std_tools = get_ide_tools_schemas();
        let gov_tools = get_governance_tools_schemas();

        match policy.mode {
            // Mode 1 (Normal) : Outils classiques ouverts
            GovernanceMode::Normal => std_tools,

            // Mode 3 (Architecture / Spec-First) : STRICTEMENT AUCUN write_file / edit_file / terminal_exec
            // Contrainte 2 : kb_search et consultation autorisés
            GovernanceMode::Architecture => {
                let allowed_std = ["read_file", "list_files", "grep_search", "workspace_map", "web_search", "web_fetch", "ask_user_question", "store_memory", "recall_memory", "recall_recent_runs", "kb_list_products", "kb_search"];
                let allowed_gov = ["add_invariant", "submit_invariants", "request_architecture_review", "symbol_search"];

                let mut tools: Vec<Value> = std_tools.into_iter()
                    .filter(|t| allowed_std.contains(&t["function"]["name"].as_str().unwrap_or("")))
                    .collect();

                let mut g: Vec<Value> = gov_tools.into_iter()
                    .filter(|t| allowed_gov.contains(&t["function"]["name"].as_str().unwrap_or("")))
                    .collect();

                tools.append(&mut g);
                tools
            }

            // Mode 2 (Contraint / Invariants & Shadow Sandbox)
            GovernanceMode::Constrained => {
                let allowed_std = ["read_file", "list_files", "grep_search", "workspace_map", "web_search", "web_fetch", "ask_user_question", "store_memory", "recall_memory", "recall_recent_runs", "kb_list_products", "kb_search", "kb_propose"];
                let allowed_gov = ["write_code", "shadow_check", "shadow_copy", "shadow_validate_and_merge", "query_invariants", "request_contract_mutation", "report_checkpoint_status", "symbol_search"];

                let mut tools: Vec<Value> = std_tools.into_iter()
                    .filter(|t| allowed_std.contains(&t["function"]["name"].as_str().unwrap_or("")))
                    .collect();

                let mut g: Vec<Value> = gov_tools.into_iter()
                    .filter(|t| allowed_gov.contains(&t["function"]["name"].as_str().unwrap_or("")))
                    .collect();

                tools.append(&mut g);
                tools
            }
        }
    }

    pub fn set_think_effort(effort: &str) {
        let lock = THINK_EFFORT.get_or_init(|| std::sync::Mutex::new("low".to_string()));
        if let Ok(mut guard) = lock.lock() {
            *guard = effort.to_string();
        }
    }

    pub fn request_cancel() {
        CANCEL_RUN.store(true, Ordering::SeqCst);
        crate::llm::client::CANCEL_LLM.store(true, Ordering::SeqCst);
    }

    pub fn run_with_callback<F>(&self, prompt: &str, mut on_trace: F) -> Result<ReActResult, String>
    where
        F: FnMut(&str, usize, &str, Option<&str>, &str, &str),
    {
        if prompt.trim() == "/recall" {
            let out = crate::hidden_command("git")
                .args(["checkout", "earnest-checkpoint-latest", "--", "."])
                .current_dir(&self.workspace_root)
                .output()
                .map_err(|e| format!("Erreur /recall git: {}", e))?;
            let msg = if out.status.success() {
                "⏪ Checkpoint restauré (`earnest-checkpoint-latest`).".to_string()
            } else {
                format!("⚠️ Aucun checkpoint trouvé ou erreur git: {}", String::from_utf8_lossy(&out.stderr))
            };
            return Ok(ReActResult {
                run_id: uuid::Uuid::new_v4().to_string(),
                status: "success".to_string(),
                answer: msg,
                steps_count: 0,
                total_tokens: 0,
                duration_ms: 0,
            });
        }

        CANCEL_RUN.store(false, Ordering::SeqCst);
        crate::llm::client::CANCEL_LLM.store(false, Ordering::SeqCst);
        let start = Instant::now();
        let run_id = format!("run_{}", &uuid::Uuid::new_v4().to_string().replace('-', "")[..12]);
        push_live_log("INFO", &format!("🚀 Run {} démarré (projet={})", run_id, self.project_id));
        self.create_checkpoint(&run_id);

        // Charger ou initialiser la politique du projet (avec garde-fou Contrainte 4)
        let pm = PolicyManager::new(&self.workspace_root);
        let policy = pm.load_or_init(None).unwrap_or_default();

        // Initialisation du run
        self.conn.execute(
            "INSERT INTO runs (id, project_id, prompt, provider, model, status, created_at) VALUES (?1, ?2, ?3, 'rust-native', 'earnest-ide-engine', 'running', CURRENT_TIMESTAMP)",
            params![run_id, self.project_id, prompt],
        ).map_err(|e| e.to_string())?;

        // Rappel automatique des mémoires du projet pour enrichir le contexte
        let mem = MemoryService::new(self.conn);
        let project_mems = mem.list_memories(&self.project_id).unwrap_or_default();
        let mut memory_context = String::new();
        if !project_mems.is_empty() {
            memory_context.push_str("\n\n[MÉMOIRES ET DÉCISIONS PERSISTÉES DU PROJET] :\n");
            for m in project_mems.iter().take(10) {
                memory_context.push_str(&format!("- {}: {}\n", m.key, m.content));
            }
        }

        // Prompt système adapté selon le Mode de gouvernance
        let mode_desc = match policy.mode {
            GovernanceMode::Normal => "MODE 1 (NORMAL) : Tu as un accès libre aux outils de développement usuels. Garde ta réflexion (thinking) très courte (quelques lignes max, JAMAIS de brouillons de code complets dedans) et passe immédiatement à l'action avec 'write_file' / 'edit_file' fichier par fichier, puis vérifie avec 'terminal_exec'.",
            GovernanceMode::Architecture => "MODE 3 (ARCHITECTURE / SPEC-FIRST) : Garde ta réflexion (thinking) très courte et factuelle. Interdiction FORMELLE d'écrire ou modifier du code source directement. Conçois uniquement l'architecture et formalise les contrats d'invariants ('add_invariant' avec id, title, statement, target_file, test_query) puis demande la validation humaine via 'request_architecture_review'.",
            GovernanceMode::Constrained => "MODE 2 (CONTRAINT PAR INVARIANTS) : Garde ta réflexion (thinking) très courte et factuelle ; passe directement aux appels d'outils. Toute modification de code passe IMPÉRATIVEMENT par 'write_code' et est validée en sandbox isolée (shadow worktree) par les sondes scellées. Modifie UN seul fichier à la fois : utilise 'target'/'replacement' pour les éditions ciblées, ou 'stage_only: true' fichier par fichier puis 'shadow_validate_and_merge' (ou un dernier 'write_code' sans 'stage_only') pour lancer les sondes et fusionner. Les fichiers déjà présents sur le disque ou stagés dans le shadow worktree sont conservés. Dès que la fusion réussit et les sondes passent, fournis ta réponse finale sans appel inutile. Si tu es bloqué par un contrat, utilise 'request_contract_mutation'.",
        };

        let workspace_map_context = match symbol_index::workspace_map(&self.workspace_root, 15, 5) {
            Ok(map) if !map.trim().is_empty() => {
                let bounded: String = map.chars().take(2_000).collect();
                format!("\n\n[CARTE WORKSPACE (symbol-index)]\n{}\n", bounded)
            }
            _ => String::new(),
        };

        let shadow_context = if policy.mode == GovernanceMode::Constrained {
            let runner = ShadowRunner::new(&self.workspace_root, &policy.shadow_env.path, policy.shadow_env.timeout_ms);
            let staged = runner.list_shadow_staged_files();
            if staged.is_empty() {
                String::new()
            } else {
                format!(
                    "\n\n[SHADOW WORKTREE ACTIF ({} fichier(s) déjà stagé(s))]\n{}\n(Utilise `shadow_check` pour compiler/tester en sandbox sans tout relire, ou `read_file` qui lit automatiquement la version shadow.)",
                    staged.len(),
                    staged.iter().take(25).cloned().collect::<Vec<_>>().join(", ")
                )
            }
        } else {
            String::new()
        };

        let earnest_md_context = if policy.mode == GovernanceMode::Normal {
            let mut parts = Vec::new();
            let global_dir = std::env::var("EARNEST_HOME")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    #[cfg(windows)]
                    {
                        std::env::var("APPDATA").ok().map(|p| std::path::PathBuf::from(p).join("Earnest"))
                    }
                    #[cfg(not(windows))]
                    {
                        std::env::var("HOME").ok().map(|p| std::path::PathBuf::from(p).join(".earnest"))
                    }
                });
            if let Some(gdir) = global_dir {
                if let Ok(raw) = std::fs::read_to_string(gdir.join("EARNEST.md")) {
                    let t = raw.trim();
                    if !t.is_empty() {
                        let bounded: String = t.chars().take(4_000).collect();
                        parts.push(format!("[EARNEST.md GLOBAL]\n{}", bounded));
                    }
                }
            }
            for rel in ["EARNEST.md", ".earnest/EARNEST.md"] {
                if let Ok(raw) = std::fs::read_to_string(self.workspace_root.join(rel)) {
                    let t = raw.trim();
                    if !t.is_empty() {
                        let bounded: String = t.chars().take(4_000).collect();
                        parts.push(format!("[EARNEST.md PROJET ({})]\n{}", rel, bounded));
                        break;
                    }
                }
            }
            if parts.is_empty() { String::new() } else { format!("\n\n{}", parts.join("\n\n")) }
        } else {
            String::new()
        };

        let system_prompt = format!(
            "Tu es Earnest IDE, un ingénieur logiciel agentique d'élite encadré par un système déterministe de gouvernance par invariants cryptographiques.\n\
            Politique active : {}\n\n\
            Règles impératives :\n\
            1. Avant de coder sur une technologie de niche, consulte 'kb_search'.\n\
            2. Examine scrupuleusement le code existant avec 'read_file' et 'list_files'.\n\
            3. En cas de bug, découverte ou piège résolu, transmets-le dans la base d'expérience via 'kb_propose'.\n\
            4. Respecte strictement les règles du mode de gouvernance actif.\n\
            5. Mémoire projet : consulte '.ide/atlas.md' avec 'read_file' s'il existe (en Mode 1 Normal, tu peux le mettre à jour aux jalons ; en Mode 2 Contraint, utilise plutôt 'store_memory' sans détourner 'write_code' pour de la documentation hors contrat).\n\
            6. En cas d'ambiguïté fonctionnelle majeure dans la demande utilisateur (plusieurs interprétations contradictoires possibles), ne boucle pas dans ta réflexion pour deviner : appelle 'ask_user_question' avec une question courte et directe.{}{}{}{}\n",
            mode_desc,
            memory_context,
            workspace_map_context,
            shadow_context,
            earnest_md_context
        );

        let mut messages = vec![
            ChatMessage { role: "system".to_string(), content: system_prompt, images: None },
        ];

        // Injection de l'historique multi-tours récent (5 derniers runs réussis du projet)
        let mut has_history = false;
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT prompt, final_answer FROM runs WHERE project_id = ?1 AND status IN ('success', 'awaiting_approval') AND final_answer IS NOT NULL AND final_answer != '' ORDER BY rowid DESC LIMIT 12"
        ) {
            if let Ok(rows) = stmt.query_map(params![self.project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            }) {
                let mut history: Vec<(String, String)> = rows.filter_map(|r| r.ok()).collect();
                history.reverse();
                has_history = !history.is_empty();
                for (prev_prompt, prev_answer) in history {
                    messages.push(ChatMessage { role: "user".to_string(), content: prev_prompt, images: None });
                    messages.push(ChatMessage { role: "assistant".to_string(), content: prev_answer, images: None });
                }
            }
        }

        let mut preload_ctx = String::new();
        if !has_history {
            if let Ok(files_val) = list_files(&self.workspace_root.to_string_lossy(), Some(2)) {
                let files_str = context_trimmer::truncate_tool_output(&files_val.to_string(), 3_000);
                preload_ctx.push_str(&format!("\n\n[CONTEXTE PROJET PRÉ-CHARGÉ — list_files depth=2]\n{}", files_str));
            }
            for manifest in ["Cargo.toml", "package.json", "pyproject.toml"] {
                let mpath = self.workspace_root.join(manifest);
                if mpath.exists() {
                    if let Ok(mcontent) = std::fs::read_to_string(&mpath) {
                        let trimmed = context_trimmer::truncate_tool_output(&mcontent, 2_000);
                        preload_ctx.push_str(&format!("\n\n[MANIFESTE {}]\n{}", manifest, trimmed));
                    }
                }
            }
        }

        messages.push(ChatMessage { role: "user".to_string(), content: format!("{}{}", prompt, preload_ctx), images: None });

        let tools = self.get_tools_for_policy(&policy);
        let mut step_order = 1;
        let mut total_tokens = 0;
        let mut final_answer = String::new();
        let mut active_backend_name = String::new();
        let mut context_budget_chars: usize = 120_000;
        let mut failure_counts: HashMap<String, usize> = HashMap::new();
        static GLOBAL_TOOL_CACHE: std::sync::OnceLock<std::sync::Mutex<tool_output_cache::ToolCache>> = std::sync::OnceLock::new();
        let cache_mutex = GLOBAL_TOOL_CACHE.get_or_init(|| std::sync::Mutex::new(tool_output_cache::ToolCache::new(128)));
        let mut tool_cache = cache_mutex.lock().unwrap_or_else(|e| e.into_inner());
        let mut llm_budget = retry_budget::RetryBudget::new(10, 5);
        let mut last_tool_sig: Option<String> = None;
        let mut repeat_count: usize = 0;
        let mut loop_detector = loop_detector::LoopDetector::new(
            loop_detector::Config::new(24, 3, 5, 2).unwrap_or_default(),
        );
        let mut empty_tool_nudges: usize = 0;
        let mut merged_successfully = false;
        let mut staged_in_run = false;

        for _ in 0..self.max_steps {
            if CANCEL_RUN.load(Ordering::SeqCst) {
                final_answer = "⏹️ Run annulé par l'utilisateur.".to_string();
                self.record_trace(&run_id, step_order, "answer", None, &final_answer);
                on_trace(&run_id, step_order, "answer", None, &final_answer, &active_backend_name);
                println!("  ⏹️ [IDE GOVERNANCE] Run annulé par l'utilisateur.");
                break;
            }
            if !llm_budget.is_retry_allowed() {
                return Err("Circuit breaker LLM ouvert : trop d'échecs consécutifs dans la fenêtre glissante.".to_string());
            }
            compact_messages_if_needed(&mut messages, context_budget_chars);
            let think_effort = THINK_EFFORT
                .get_or_init(|| std::sync::Mutex::new("low".to_string()))
                .lock()
                .map(|g| g.clone())
                .unwrap_or_else(|_| "low".to_string());
            let mut stream_cb = |kind: &str, chunk: &str| {
                on_trace(&run_id, step_order, "token", Some(kind), chunk, &active_backend_name);
            };
            let (chat_res, active_backend) = match self.router.chat_completion_with_fallback_stream(&messages, Some(&tools), &think_effort, &mut stream_cb) {
                Ok(res) => {
                    llm_budget.record_success();
                    res
                }
                Err(e) => {
                    llm_budget.record_failure();
                    return Err(e);
                }
            };
            if active_backend != active_backend_name {
                context_budget_chars = self.router.estimate_context_chars(&active_backend);
                println!("  🤖 [LLM] Backend actif : {} (budget contexte: {} chars)", active_backend, context_budget_chars);
                push_live_log("STEP", &format!("🤖 Tour {} [{}] (budget {} chars)", step_order + 1, active_backend, context_budget_chars));
            }
            active_backend_name = active_backend;
            total_tokens += chat_res.total_tokens;

            if let Some(ref rc) = chat_res.reasoning_content {
                if !rc.trim().is_empty() {
                    self.record_trace(&run_id, step_order, "thought", None, rc);
                    on_trace(&run_id, step_order, "thought", None, rc, &active_backend_name);
                    step_order += 1;
                }
            }

            let truncated_in_thought = chat_res.content.trim().is_empty()
                && chat_res.reasoning_content.as_deref().map(|r| !r.trim().is_empty()).unwrap_or(false);
            let has_code_fence = chat_res.content.contains("```");
            let unfinished_announcement = react_resilience::looks_like_unfinished_thought(&chat_res.content);
            let normal_stalled = chat_res.tool_calls.is_empty()
                && policy.mode == GovernanceMode::Normal
                && empty_tool_nudges < 5
                && (chat_res.content.matches("```").count() % 2 == 1
                    || truncated_in_thought
                    || unfinished_announcement);
            let arch_stalled = chat_res.tool_calls.is_empty()
                && policy.mode == GovernanceMode::Architecture
                && empty_tool_nudges < 5;
            let constrained_stalled = chat_res.tool_calls.is_empty()
                && policy.mode == GovernanceMode::Constrained
                && !merged_successfully
                && empty_tool_nudges < 5
                && (truncated_in_thought || has_code_fence || staged_in_run || unfinished_announcement);
            let is_final_answer = chat_res.tool_calls.is_empty()
                && !normal_stalled
                && !arch_stalled
                && !constrained_stalled;

            if !chat_res.content.is_empty() {
                if !is_final_answer {
                    self.record_trace(&run_id, step_order, "thought", None, &chat_res.content);
                    on_trace(&run_id, step_order, "thought", None, &chat_res.content, &active_backend_name);
                    step_order += 1;
                }
                messages.push(ChatMessage { role: "assistant".to_string(), content: chat_res.content.clone(), images: None });
            }

            // S'il n'y a aucun appel d'outil, conclusion (tolérance 5 tours de réflexion/transition avant arrêt)
            if chat_res.tool_calls.is_empty() {
                if normal_stalled {
                    empty_tool_nudges += 1;
                    let has_unclosed_fence = chat_res.content.matches("```").count() % 2 == 1;
                    println!("  🔄 [ANTI-STALL] Transition/réflexion sans outil en Mode 1 ({}/5 consécutifs), relance automatique...", empty_tool_nudges);
                    let msg = if has_unclosed_fence || truncated_in_thought {
                        "Ta réponse précédente a été tronquée avant d'appeler un outil. N'écris pas le code en texte libre : appelle immédiatement `write_file` (avec `append: true` si tu écris un gros fichier en plusieurs morceaux) ou `edit_file`."
                    } else {
                        "Poursuis : si tu as fini ta réflexion intermédiaire, appelle immédiatement l'outil suivant (`terminal_exec`, `edit_file`, `write_file`, `read_file`...). Si toute la mission est réellement terminée et vérifiée, fournis ton bilan complet final."
                    };
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: msg.to_string(),
                        images: None,
                    });
                    continue;
                }
                if arch_stalled {
                    empty_tool_nudges += 1;
                    println!("  🔄 [ANTI-STALL] Réflexion sans outil en Mode 3 ({}/5 consécutifs), relance automatique...", empty_tool_nudges);
                    let msg = if chat_res.content.is_empty() {
                        "Ta réflexion a été tronquée avant d'appeler un outil. Quand ton plan est prêt, appelle `add_invariant` pour chaque contrat, puis `request_architecture_review`."
                    } else {
                        "En Mode 3 Architecture, dès que ta réflexion d'architecture est prête, appelle `add_invariant` pour formaliser chaque contrat (id, title, statement, target_file, test_query), puis `request_architecture_review` pour soumettre le plan à validation (ou `ask_user_question` si la consigne est ambiguë)."
                    };
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: msg.to_string(),
                        images: None,
                    });
                    continue;
                }
                if constrained_stalled {
                    empty_tool_nudges += 1;
                    println!("  🔄 [ANTI-STALL] Transition/réflexion sans outil en Mode 2 ({}/5 consécutifs), relance automatique...", empty_tool_nudges);
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: "Poursuis : appelle l'outil suivant (`write_code`, `shadow_check`, ou `shadow_validate_and_merge` si les fichiers sont déjà stagés dans le shadow worktree, ou `ask_user_question` en cas d'ambiguïté).".to_string(),
                        images: None,
                    });
                    continue;
                }
                final_answer = chat_res.content;
                self.record_trace(&run_id, step_order, "answer", None, &final_answer);
                on_trace(&run_id, step_order, "answer", None, &final_answer, &active_backend_name);
                break;
            }
            empty_tool_nudges = 0;

            // Exécution des outils demandés (parallèle si plusieurs appels read-only)
            let all_readonly_batch = chat_res.tool_calls.len() > 1
                && chat_res.tool_calls.iter().all(|c| matches!(c.name.as_str(), "read_file" | "list_files" | "grep_search"));

            let batch_results: Vec<(String, Option<String>)> = if all_readonly_batch {
                let mut results: Vec<Option<(String, Option<String>)>> = vec![None; chat_res.tool_calls.len()];
                let mut misses: Vec<(usize, &str, &Value)> = Vec::new();
                for (idx, call) in chat_res.tool_calls.iter().enumerate() {
                    if call.name == "read_file" {
                        let args_key = normalize_cache_args_key(&self.workspace_root, &call.arguments);
                        if let Some(cached) = tool_cache.get(&call.name, &args_key) {
                            println!("  ♻️ [TOOL CACHE] HIT {} {}", call.name, args_key);
                            results[idx] = Some((cached.to_string(), None));
                            continue;
                        }
                    }
                    misses.push((idx, call.name.as_str(), &call.arguments));
                }
                if !misses.is_empty() {
                    let computed: Vec<(usize, (String, Option<String>))> = std::thread::scope(|s| {
                        let ws_root = self.workspace_root.as_path();
                        let policy_ref = &policy;
                        let handles: Vec<_> = misses.into_iter().map(|(idx, name, args)| {
                            s.spawn(move || {
                                let out = Self::exec_readonly_tool(ws_root, name, args, policy_ref);
                                (idx, (out, None))
                            })
                        }).collect();
                        handles.into_iter().map(|h| h.join().unwrap_or_else(|_| (0, ("{\"status\":\"error\",\"error\":\"thread panic\"}".to_string(), None)))).collect()
                    });
                    for (idx, res) in computed {
                        results[idx] = Some(res);
                    }
                }
                results.into_iter().map(|r| r.unwrap_or_else(|| (String::new(), None))).collect()
            } else {
                Vec::new()
            };

            for (call_idx, call) in chat_res.tool_calls.into_iter().enumerate() {
                let fn_name = &call.name;
                let fn_args = &call.arguments;

                self.record_trace(&run_id, step_order, "tool_call", Some(fn_name), &fn_args.to_string());
                on_trace(&run_id, step_order, "tool_call", Some(fn_name), &fn_args.to_string(), &active_backend_name);
                step_order += 1;

                println!("  ⚡ [GOUVERNANCE IDE] {} {:?}", fn_name, fn_args);
                push_live_log("TOOL", &format!("⚙ Appel {}({})", fn_name, context_trimmer::truncate_tool_output(&fn_args.to_string(), 180)));
                let args_key = normalize_cache_args_key(&self.workspace_root, fn_args);
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash(&args_key, &mut hasher);
                let args_hash = std::hash::Hasher::finish(&hasher);
                loop_detector.observe(fn_name, args_hash);
                let sig = format!("{}:{}", fn_name, args_key);
                if last_tool_sig.as_deref() == Some(&sig) {
                    repeat_count += 1;
                } else {
                    last_tool_sig = Some(sig);
                    repeat_count = 1;
                }

                let is_cacheable_read = matches!(fn_name.as_str(), "read_file");

                let (raw_output, interrupt_action) = if all_readonly_batch {
                    batch_results[call_idx].clone()
                } else if is_cacheable_read {
                    if let Some(cached) = tool_cache.get(fn_name, &args_key) {
                        println!("  ♻️ [TOOL CACHE] HIT {} {}", fn_name, args_key);
                        (cached.to_string(), None)
                    } else {
                        self.dispatch_tool(fn_name, fn_args, &policy, &mut failure_counts)
                    }
                } else {
                    self.dispatch_tool(fn_name, fn_args, &policy, &mut failure_counts)
                };

                let sanitized_output = log_sanitizer::sanitize_text(&raw_output);
                let mut tool_output = context_trimmer::truncate_tool_output(&sanitized_output, 16_000);
                push_live_log("OUTPUT", &format!("← {} : {}", fn_name, context_trimmer::truncate_tool_output(&tool_output, 220)));
                if repeat_count >= 3 {
                    tool_output.push_str("\n\n[ANTI-LOOP] Attention : tu viens de répéter exactement le même appel 3 fois de suite. Change d'approche, passe à l'étape suivante ou conclus.");
                    repeat_count = 0;
                } else if let loop_detector::Verdict::LoopDetected { rule, confidence } = loop_detector.evaluate() {
                    tool_output.push_str(&format!("\n\n[ANTI-LOOP:{}] Motif répétitif ou cyclique détecté (score {}). Change d'approche ou passe à l'étape suivante.", rule, confidence));
                    loop_detector.reset_progress();
                }

                let is_tool_error = raw_output.contains("\"status\":\"error\"") || raw_output.contains("\"status\": \"error\"");
                if is_cacheable_read && interrupt_action.is_none() && !is_tool_error {
                    let path_hint = match fn_name.as_str() {
                        "read_file" => fn_args["path"].as_str(),
                        "list_files" | "grep_search" => fn_args["root_path"].as_str().or(Some(".")),
                        _ => None,
                    };
                    tool_cache.insert(fn_name, &args_key, tool_output.clone(), path_hint);
                } else if matches!(fn_name.as_str(), "add_invariant" | "submit_invariants") {
                    loop_detector.reset_progress();
                    tool_cache.invalidate_path(&policy.invariants_file);
                    tool_cache.invalidate_path("invariants.json");
                } else if matches!(fn_name.as_str(), "write_code" | "edit_file" | "write_file" | "shadow_copy" | "shadow_validate_and_merge") {
                    loop_detector.reset_progress();
                    if matches!(fn_name.as_str(), "write_code" | "shadow_copy") && raw_output.contains("\"status\":\"STAGED\"") {
                        staged_in_run = true;
                    }
                    if matches!(fn_name.as_str(), "write_code" | "shadow_validate_and_merge") && raw_output.contains("\"merged_to_workspace\":true") {
                        merged_successfully = true;
                        staged_in_run = false;
                    }
                    if let Some(path) = fn_args["path"].as_str().or_else(|| fn_args["target_file"].as_str()) {
                        tool_cache.invalidate_path(path);
                    }
                    if let Some(files_arr) = fn_args["files"].as_array() {
                        for item in files_arr {
                            if let Some(p) = item["target_file"].as_str().or_else(|| item["path"].as_str()) {
                                tool_cache.invalidate_path(p);
                            }
                        }
                    }
                }

                if interrupt_action.as_deref() == Some("user_question") {
                    final_answer = raw_output;
                    self.record_trace(&run_id, step_order, "answer", None, &final_answer);
                    on_trace(&run_id, step_order, "answer", None, &final_answer, &active_backend_name);
                    break;
                }

                self.record_trace(&run_id, step_order, "tool_output", Some(fn_name), &tool_output);
                on_trace(&run_id, step_order, "tool_output", Some(fn_name), &tool_output, &active_backend_name);
                step_order += 1;

                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: format_tool_observation(fn_name, fn_args, &tool_output),
                    images: None,
                });

                // Contrainte 5 : Interruption propre de la boucle ReAct si une action humaine est requise
                if let Some(action) = interrupt_action {
                    let duration_ms = start.elapsed().as_millis();
        let cache_stats = tool_cache.stats();
        let budget_stats = llm_budget.stats();
        println!(
            "  📊 [TELEMETRY] ToolCache(hits={}, misses={}, len={}) | RetryBudget(failures={}, open={})",
            cache_stats.hits, cache_stats.misses, cache_stats.len, budget_stats.failures, budget_stats.is_open
        );
                    println!("  ⏸️ [GOUVERNANCE IDE] Interruption (awaiting_approval:{}, {}ms)", action, duration_ms);
                    let saved_interrupt = context_trimmer::truncate_tool_output(&tool_output, 4_000);
                    self.conn.execute(
                        "UPDATE runs SET status = 'awaiting_approval', completed_at = CURRENT_TIMESTAMP, duration_ms = ?1, total_tokens = ?2, final_answer = ?4 WHERE id = ?3",
                        params![duration_ms as i64, total_tokens as i64, run_id, saved_interrupt],
                    ).ok();

                    return Ok(ReActResult {
                        run_id,
                        status: format!("awaiting_approval:{}", action),
                        answer: tool_output,
                        steps_count: step_order,
                        total_tokens,
                        duration_ms,
                    });
                }
            }
        }

        let duration_ms = start.elapsed().as_millis();
        let cache_stats = tool_cache.stats();
        let budget_stats = llm_budget.stats();
        println!(
            "  📊 [TELEMETRY] ToolCache(hits={}, misses={}, len={}) | RetryBudget(failures={}, open={})",
            cache_stats.hits, cache_stats.misses, cache_stats.len, budget_stats.failures, budget_stats.is_open
        );

        if final_answer.is_empty() {
            final_answer = format!(
                "⚠️ Reached max step limit ({}) without a final answer. You can ask me to continue.",
                self.max_steps
            );
            self.record_trace(&run_id, step_order, "answer", None, &final_answer);
            on_trace(&run_id, step_order, "answer", None, &final_answer, &active_backend_name);
            println!("  ⚠️ [IDE GOVERNANCE] Max steps limit ({}) reached ({}ms)", self.max_steps, duration_ms);
        } else {
            println!("  ✅ [IDE GOVERNANCE] Run completed ({} steps, {}ms)", step_order, duration_ms);
            push_live_log("DONE", &format!("✅ Run {} terminé ({} étapes, {}ms)", run_id, step_order, duration_ms));
        }

        self.run_ide_hooks("on_run_complete");

        let saved_answer = context_trimmer::truncate_tool_output(&final_answer, 4_000);
        self.conn.execute(
            "UPDATE runs SET status = 'success', completed_at = CURRENT_TIMESTAMP, duration_ms = ?1, total_tokens = ?2, final_answer = ?4 WHERE id = ?3",
            params![duration_ms as i64, total_tokens as i64, run_id, saved_answer],
        ).ok();

        Ok(ReActResult {
            run_id,
            status: "success".to_string(),
            answer: final_answer,
            steps_count: step_order,
            total_tokens,
            duration_ms,
        })
    }

    pub fn run(&self, prompt: &str) -> Result<ReActResult, String> {
        self.run_with_callback(prompt, |_, _, _, _, _, _| {})
    }

    fn create_checkpoint(&self, run_id: &str) {
        if !self.workspace_root.join(".git").exists() {
            return;
        }
        if let Ok(out) = crate::hidden_command("git")
            .args(["stash", "create", "earnest-checkpoint"])
            .current_dir(&self.workspace_root)
            .output()
        {
            let hash = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let target = if hash.is_empty() { "HEAD" } else { hash.as_str() };
            let _ = crate::hidden_command("git")
                .args(["tag", "-f", "earnest-checkpoint-latest", target])
                .current_dir(&self.workspace_root)
                .output();
            let ref_name = format!("refs/earnest/checkpoints/{}", run_id);
            let _ = crate::hidden_command("git")
                .args(["update-ref", &ref_name, target])
                .current_dir(&self.workspace_root)
                .output();
            println!("  🔖 [CHECKPOINT] Sauvegardé ({} -> {})", run_id, target);
            push_live_log("INFO", &format!("🔖 Checkpoint {} ({})", run_id, target));
        }
    }

    fn run_ide_hooks(&self, event: &str) {
        let hooks_path = self.workspace_root.join(".ide/hooks.json");
        let Ok(content) = std::fs::read_to_string(&hooks_path) else { return };
        let Ok(val) = serde_json::from_str::<Value>(&content) else { return };
        let Some(cmds) = val[event].as_array() else { return };
        for cmd_val in cmds {
            let Some(cmd) = cmd_val.as_str() else { continue };
            println!("  🪝 [HOOK {}] {}", event, cmd);
            let status = if cfg!(target_os = "windows") {
                crate::hidden_command("cmd").args(["/C", cmd]).current_dir(&self.workspace_root).status()
            } else {
                crate::hidden_command("sh").args(["-c", cmd]).current_dir(&self.workspace_root).status()
            };
            if let Ok(s) = status {
                if !s.success() {
                    println!("  ⚠️ [HOOK {}] '{}' exited with {}", event, cmd, s);
                }
            }
        }
    }
}

pub struct SubAgentRunner<'a> {
    pub conn: &'a Connection,
    pub router: &'a LLMRouter,
    pub project_id: String,
    pub role: String,
    pub readonly: bool,
    pub max_steps: usize,
}

impl<'a> SubAgentRunner<'a> {
    fn dispatch_sub_tool(&self, name: &str, args: &Value) -> String {
        if self.readonly && (name == "edit_file" || name == "write_file" || name == "terminal_exec" || name == "write_code") {
            return serde_json::json!({
                "status": "error",
                "error": format!("Outil '{}' interdit : ce sous-agent opère en mode lecture seule (read-only sandbox)", name)
            }).to_string();
        }

        let ws_root = if Path::new(&self.project_id).is_dir() {
            PathBuf::from(&self.project_id)
        } else {
            PathBuf::from(".")
        };
        let resolve_ws = |raw: &str| -> String {
            if Path::new(raw).is_absolute() {
                raw.to_string()
            } else {
                ws_root.join(raw).to_string_lossy().to_string()
            }
        };
        match name {
            "read_file" => {
                let path = resolve_ws(args["path"].as_str().unwrap_or(""));
                let start_line = args["start_line"].as_u64().map(|v| v as usize);
                let end_line = args["end_line"].as_u64().map(|v| v as usize);
                match read_file(&path, start_line, end_line) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "grep_search" => {
                let query = args["query"].as_str().unwrap_or("");
                let root = resolve_ws(args["root_path"].as_str().unwrap_or("."));
                let max = args["max_results"].as_u64().map(|v| v as usize);
                match grep_search(&root, query, max) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "list_files" => {
                let root = resolve_ws(args["root_path"].as_str().unwrap_or("."));
                let depth = args["max_depth"].as_u64().map(|v| v as usize);
                match list_files(&root, depth) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "web_search" => {
                let query = args["query"].as_str().unwrap_or("");
                let max = args["max_results"].as_u64().unwrap_or(5) as usize;
                match search_web(query, max) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "terminal_exec" => {
                let cmd = args["command"].as_str().unwrap_or("");
                let timeout = args["timeout_sec"].as_u64().unwrap_or(60);
                match execute_terminal(cmd, timeout) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            _ => serde_json::json!({"status": "error", "error": format!("Outil non supporté pour sous-agent : {}", name)}).to_string(),
        }
    }

    pub fn run(&self, mission: &str) -> Result<String, String> {
        let sub_run_id = format!("sub_{}", &uuid::Uuid::new_v4().to_string().replace('-', "")[..10]);

        self.conn.execute(
            "INSERT INTO runs (id, project_id, prompt, provider, model, status, created_at) VALUES (?1, ?2, ?3, 'subagent', ?4, 'running', CURRENT_TIMESTAMP)",
            params![sub_run_id, self.project_id, mission, self.role],
        ).ok();

        let sys_prompt = format!(
            "Tu es un SOUS-AGENT spécialisé avec le rôle : '{}'. \
            Tu es invoqué par l'agent coordinateur principal de Earnest IDE pour accomplir une mission ciblée. \
            Mode sandbox : {}. \
            Fouille, explore et synthétise tes résultats avec une extrême rigueur. \
            Ta réponse finale DOIT être un rapport de synthèse ultra-clair, direct et concis (sans bavardage) répondant exactement à la mission.",
            self.role,
            if self.readonly { "LECTURE SEULE (seuls les outils de lecture/recherche sont autorisés)" } else { "STANDARD" }
        );

        let mut messages = vec![
            ChatMessage { role: "system".to_string(), content: sys_prompt, images: None },
            ChatMessage { role: "user".to_string(), content: mission.to_string(), images: None },
        ];

        let all_tools = get_ide_tools_schemas();
        let sub_tools: Vec<Value> = all_tools.into_iter()
            .filter(|t| {
                let n = t["function"]["name"].as_str().unwrap_or("");
                n != "invoke_subagent" && n != "ask_user_question" && (!self.readonly || (n != "edit_file" && n != "write_file" && n != "terminal_exec"))
            })
            .collect();

        let mut final_report = String::new();

        for _ in 0..self.max_steps {
            let (chat_res, _) = self.router.chat_completion_with_fallback(&messages, Some(&sub_tools))?;

            if chat_res.tool_calls.is_empty() {
                final_report = chat_res.content;
                break;
            }

            if !chat_res.content.is_empty() {
                messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: chat_res.content,
                    images: None,
                });
            }

            for call in chat_res.tool_calls {
                let fn_name = &call.name;
                let fn_args = &call.arguments;

                let output = self.dispatch_sub_tool(fn_name, fn_args);
                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: format_tool_observation(fn_name, fn_args, &output),
                    images: None,
                });
            }
        }

        self.conn.execute(
            "UPDATE runs SET status = 'success', completed_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![sub_run_id],
        ).ok();

        if final_report.is_empty() {
            final_report = "Mission accomplie sans synthèse textuelle explicite.".to_string();
        }

        Ok(final_report)
    }
}

fn format_tool_observation(fn_name: &str, fn_args: &Value, tool_output: &str) -> String {
    let args_preview = context_trimmer::truncate_tool_output(&fn_args.to_string(), 1_200);
    format!(
        "Résultat de l'outil '{}' (arguments soumis: {}) : {}",
        fn_name, args_preview, tool_output
    )
}

fn normalize_cache_args_key(workspace_root: &Path, args: &Value) -> String {
    let mut cloned = args.clone();
    if let Some(obj) = cloned.as_object_mut() {
        let ws = workspace_root.to_string_lossy().replace('\\', "/");
        let ws_trimmed = ws.trim_end_matches('/');
        for key in ["path", "root_path"] {
            if let Some(val) = obj.get(key).and_then(|v| v.as_str()).map(|s| s.replace('\\', "/")) {
                let rel = val.strip_prefix(ws_trimmed).map(|r| r.trim_start_matches('/')).unwrap_or(&val);
                let clean = rel.strip_prefix("./").unwrap_or(rel);
                let norm = if clean.is_empty() { "." } else { clean };
                obj.insert(key.to_string(), serde_json::json!(norm));
            }
        }
    }
    cloned.to_string()
}

/// Compacte l'historique via la brique partagée `react_resilience` (Rolling Working Memory + budget de tokens).
fn compact_messages_if_needed(messages: &mut Vec<ChatMessage>, max_chars: usize) {
    let mut tmp: Vec<react_resilience::CompactionMessage> = messages
        .iter()
        .map(|m| react_resilience::CompactionMessage {
            role: m.role.clone(),
            content: m.content.clone(),
            has_images: m.images.is_some(),
        })
        .collect();
    react_resilience::compact_with_rolling_memory(&mut tmp, max_chars);
    react_resilience::compact_by_tokens(&mut tmp, max_chars);
    if tmp.len() == messages.len()
        && tmp.iter().zip(messages.iter()).all(|(a, b)| a.role == b.role && a.content == b.content)
    {
        return;
    }
    *messages = tmp
        .into_iter()
        .map(|m| ChatMessage {
            role: m.role,
            content: m.content,
            images: None,
        })
        .collect();
}

fn kb_mcp_client() -> crate::mcp::McpClient {
    let kb_exe = crate::tools::resolve_kb_exe_path();
    let kb_db = crate::tools::resolve_kb_db_path();
    crate::mcp::McpClient::new(
        &kb_exe.to_string_lossy(),
        &[
            "serve".to_string(),
            "--db".to_string(),
            kb_db.to_string_lossy().to_string(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_tool_observation_includes_submitted_args_without_assistant_imitation() {
        let args = serde_json::json!({
            "target_contract_id": "C3",
            "target_file": "src/engine.js",
            "target": "old_block",
            "replacement": "new_block"
        });
        let out = format_tool_observation("write_code", &args, "{\"status\":\"SUCCESS\"}");
        assert!(out.contains("Résultat de l'outil 'write_code'"));
        assert!(out.contains("src/engine.js"));
        assert!(out.contains("new_block"));
        assert!(!out.contains("[Appel outil:"));
    }

    #[test]
    fn exec_readonly_tool_prefers_shadow_worktree_in_constrained_mode() {
        let tmp = std::env::temp_dir().join(format!("earnest_ide_shadow_batch_{}", uuid::Uuid::new_v4()));
        let shadow_dir = tmp.join(".ide/shadow_worktree");
        std::fs::create_dir_all(&shadow_dir).unwrap();
        std::fs::write(tmp.join("package.json"), "{\"version\":\"1.0.0\"}").unwrap();
        std::fs::write(shadow_dir.join("package.json"), "{\"version\":\"2.0.0-shadow\"}").unwrap();

        let mut policy = PolicyConfig::default();
        policy.mode = GovernanceMode::Constrained;

        let abs_args = serde_json::json!({ "path": tmp.join("package.json").to_string_lossy() });
        let out = ReActEngine::exec_readonly_tool(&tmp, "read_file", &abs_args, &policy);
        assert!(out.contains("2.0.0-shadow"), "Expected shadow content, got: {}", out);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ask_user_question_formats_clean_interrupt() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let router = LLMRouter::from_db(&conn);
        let engine = ReActEngine::new(&conn, &router, "test_proj");
        let policy = PolicyConfig::default();
        let mut failures = HashMap::new();

        let args = serde_json::json!({
            "question": "Le rocher immobile au-dessus du joueur doit-il tomber dès que le joueur quitte la case ?",
            "options": ["Oui, dès le tick suivant", "Non, il reste bloqué"]
        });
        let (msg, interrupt) = engine.dispatch_tool("ask_user_question", &args, &policy, &mut failures);
        assert_eq!(interrupt.as_deref(), Some("user_question"));
        assert!(msg.contains("Clarification souhaitée"));
        assert!(msg.contains("1. Oui, dès le tick suivant"));
    }
}
