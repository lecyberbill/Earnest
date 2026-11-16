use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

static CANCEL_RUN: AtomicBool = AtomicBool::new(false);
static THINK_EFFORT: std::sync::OnceLock<std::sync::Mutex<String>> = std::sync::OnceLock::new();
use rusqlite::{params, Connection};
use serde_json::Value;

use crate::db::memories::MemoryService;
use crate::llm::client::ChatMessage;
use crate::llm::router::LLMRouter;
use crate::tools::fs::{read_file, write_file, edit_file};
use crate::tools::search::{grep_search, list_files};
use crate::tools::terminal::execute_terminal;
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
                let target_file = args["target_file"].as_str().unwrap_or("");
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
                        return Ok(raw_patch.to_string());
                    }
                    if let (Some(target), Some(replacement)) = (item["target"].as_str(), item["replacement"].as_str()) {
                        let abs_path = self.workspace_root.join(file_rel);
                        let existing = std::fs::read_to_string(&abs_path)
                            .map_err(|e| format!("Édition ciblée impossible sur '{}': {}", file_rel, e))?;
                        if !existing.contains(target) {
                            return Err(format!("Bloc 'target' introuvable dans '{}' pour l'édition ciblée.", file_rel));
                        }
                        return Ok(existing.replacen(target, replacement, 1));
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
                            let f = item["target_file"].as_str().or_else(|| item["path"].as_str()).unwrap_or("");
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
                        let f = item["target_file"].as_str().or_else(|| item["path"].as_str()).unwrap_or("");
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
                let _ = std::fs::write(&inv_path, json_str);
                return (serde_json::json!({
                    "status": "ok",
                    "contract_id": cid,
                    "total_contracts": total,
                    "message": "Contract added to invariants.json. Call request_architecture_review when all contracts are ready."
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
                let hash = args["invariants_hash"].as_str().unwrap_or("");
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
            "read_file" => {
                let path = args["path"].as_str().unwrap_or("");
                let resolved = if Path::new(path).is_absolute() {
                    PathBuf::from(path)
                } else {
                    self.workspace_root.join(path)
                };
                let start_line = args["start_line"].as_u64().map(|v| v as usize);
                let end_line = args["end_line"].as_u64().map(|v| v as usize);
                match read_file(resolved.to_str().unwrap_or(path), start_line, end_line) {
                    Ok(res) => (res.to_string(), None),
                    Err(e) => (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                }
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
                match write_file(resolved.to_str().unwrap_or(path), content) {
                    Ok(res) => (res.to_string(), None),
                    Err(e) => (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                }
            }
            "grep_search" => {
                let query = args["query"].as_str().unwrap_or("");
                let raw_root = args["root_path"].as_str().unwrap_or(".");
                let resolved_root = if Path::new(raw_root).is_absolute() {
                    PathBuf::from(raw_root)
                } else {
                    self.workspace_root.join(raw_root)
                };
                let root = resolved_root.to_str().unwrap_or(raw_root);
                let max = args["max_results"].as_u64().map(|v| v as usize);
                match grep_search(root, query, max) {
                    Ok(res) => (res.to_string(), None),
                    Err(e) => (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                }
            }
            "list_files" => {
                let raw_root = args["root_path"].as_str().unwrap_or(".");
                let resolved_root = if Path::new(raw_root).is_absolute() {
                    PathBuf::from(raw_root)
                } else {
                    self.workspace_root.join(raw_root)
                };
                let root = resolved_root.to_str().unwrap_or(raw_root);
                let depth = args["max_depth"].as_u64().map(|v| v as usize);
                match list_files(root, depth) {
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
                match execute_terminal(cmd, timeout) {
                    Ok(res) => (res.to_string(), None),
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
                        "SELECT DISTINCT r.id, r.status, r.prompt, COALESCE(r.final_answer, ''), r.created_at FROM runs r LEFT JOIN traces t ON t.run_id = r.id WHERE r.project_id = ?1 AND (r.prompt LIKE ?2 OR r.final_answer LIKE ?2 OR t.content LIKE ?2) ORDER BY r.rowid DESC LIMIT ?3"
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
            "kb_search" => {
                let pid = args["product_id"].as_str().unwrap_or("earnest");
                let q = args["query"].as_str().unwrap_or("");
                let mcp = crate::mcp::McpClient::new(
                    [
                        r"earnest-bcpe\target\release\kb.exe",
                        r"..\earnest-bcpe\target\release\kb.exe",
                        r"..\..\earnest-bcpe\target\release\kb.exe",
                        r"D:\Projet\Hernest\earnest-bcpe\target\release\kb.exe",
                    ]
                    .into_iter()
                    .find(|p| std::path::Path::new(p).exists())
                    .unwrap_or(r"earnest-bcpe\target\release\kb.exe"),
                    &["serve".to_string(), "--db".to_string(), r"earnest-bcpe\kb.db".to_string()]
                );
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

                let mcp = crate::mcp::McpClient::new(
                    [
                        r"earnest-bcpe\target\release\kb.exe",
                        r"..\earnest-bcpe\target\release\kb.exe",
                        r"..\..\earnest-bcpe\target\release\kb.exe",
                        r"D:\Projet\Hernest\earnest-bcpe\target\release\kb.exe",
                    ]
                    .into_iter()
                    .find(|p| std::path::Path::new(p).exists())
                    .unwrap_or(r"earnest-bcpe\target\release\kb.exe"),
                    &["serve".to_string(), "--db".to_string(), r"earnest-bcpe\kb.db".to_string()]
                );

                mcp.call_tool("kb_search", &serde_json::json!({ "product_id": pid, "query": subject_key })).ok();

                let mut propose_args = serde_json::json!({
                    "product_id": pid,
                    "subject_key": subject_key,
                    "kind": kind,
                    "source": "agent_discovery",
                    "title": title,
                    "correction": correction,
                    "auto_validate": true
                });

                if let Some(s) = symptom { propose_args["symptom"] = serde_json::json!(s); }
                if let Some(c) = cause { propose_args["cause"] = serde_json::json!(c); }
                if let Some(e) = example { propose_args["example"] = serde_json::json!(e); }

                match mcp.call_tool("kb_propose", &propose_args) {
                    Ok(res) => (res.to_string(), None),
                    Err(e) => (serde_json::json!({"status": "error", "error": e}).to_string(), None),
                }
            }
            _ => (serde_json::json!({"status": "error", "error": format!("Outil inconnu : {}", name)}).to_string(), None),
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
                let allowed_std = ["read_file", "list_files", "grep_search", "web_search", "web_fetch", "store_memory", "recall_memory", "recall_recent_runs", "kb_search"];
                let allowed_gov = ["add_invariant", "submit_invariants", "request_architecture_review"];

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
                let allowed_std = ["read_file", "list_files", "grep_search", "web_search", "web_fetch", "store_memory", "recall_memory", "recall_recent_runs", "kb_search", "kb_propose"];
                let allowed_gov = ["write_code", "query_invariants", "request_contract_mutation", "report_checkpoint_status"];

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
        let lock = THINK_EFFORT.get_or_init(|| std::sync::Mutex::new("medium".to_string()));
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
            let out = std::process::Command::new("git")
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
        self.create_checkpoint();
        let start = Instant::now();
        let run_id = format!("run_{}", &uuid::Uuid::new_v4().to_string().replace('-', "")[..12]);

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
            GovernanceMode::Normal => "MODE 1 (NORMAL) : Tu as un accès libre aux outils de développement usuels.",
            GovernanceMode::Architecture => "MODE 3 (ARCHITECTURE / SPEC-FIRST) : Interdiction FORMELLE d'écrire ou modifier du code source directement. Tu dois concevoir l'architecture et formaliser les contrats d'invariants (préfère l'outil plat 'add_invariant' avec id, title, statement, target_file, test_query) puis demander la validation humaine via 'request_architecture_review'.",
            GovernanceMode::Constrained => "MODE 2 (CONTRAINT PAR INVARIANTS) : Toute modification de code passe IMPÉRATIVEMENT par 'write_code' et est validée en sandbox isolée (shadow worktree) par les sondes cryptographiques scellées. Sur un nouveau projet, le shadow worktree est réinitialisé à chaque 'write_code' : inclus dans le même appel (via 'files') ou via 'stage_only: true' fichier par fichier puis un appel final sans stage_only. N'émets JAMAIS de message texte seul sans appel d'outil tant que tous les contrats ne sont pas validés par 'write_code'. Si tu es bloqué par un contrat, utilise 'request_contract_mutation'.",
        };

        let system_prompt = format!(
            "Tu es Earnest IDE, un ingénieur logiciel agentique d'élite encadré par un système déterministe de gouvernance par invariants cryptographiques.\n\
            Politique active : {}\n\n\
            Règles impératives :\n\
            1. Avant de coder sur une technologie de niche, consulte 'kb_search'.\n\
            2. Examine scrupuleusement le code existant avec 'read_file' et 'list_files'.\n\
            3. En cas de bug, découverte ou piège résolu, transmets-le dans la base d'expérience via 'kb_propose'.\n\
            4. Respecte strictement les règles du mode de gouvernance actif.\n\
            5. Atlas projet : maintiens '.ide/atlas.md' (carte des modules, APIs publiques, décisions) aux jalons et consulte-le avec 'read_file' quand tu as besoin de contexte.{}\n",
            mode_desc,
            memory_context
        );

        let mut messages = vec![
            ChatMessage { role: "system".to_string(), content: system_prompt, images: None },
        ];

        // Injection de l'historique multi-tours récent (5 derniers runs réussis du projet)
        let mut has_history = false;
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT prompt, final_answer FROM runs WHERE project_id = ?1 AND status = 'success' AND final_answer IS NOT NULL AND final_answer != '' ORDER BY rowid DESC LIMIT 12"
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
        let mut empty_tool_nudges: usize = 0;

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
                .get_or_init(|| std::sync::Mutex::new("medium".to_string()))
                .lock()
                .map(|g| g.clone())
                .unwrap_or_else(|_| "medium".to_string());
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
            }
            active_backend_name = active_backend;
            total_tokens += chat_res.total_tokens;

            if let Some(ref rc) = chat_res.reasoning_content {
                if !rc.is_empty() {
                    on_trace(&run_id, step_order, "thought", None, rc, &active_backend_name);
                }
            }

            if !chat_res.content.is_empty() {
                self.record_trace(&run_id, step_order, "thought", None, &chat_res.content);
                on_trace(&run_id, step_order, "thought", None, &chat_res.content, &active_backend_name);
                step_order += 1;
                messages.push(ChatMessage { role: "assistant".to_string(), content: chat_res.content.clone(), images: None });
            }

            // S'il n'y a aucun appel d'outil, conclusion (sauf si Mode 2 et que le modèle s'est arrêté en cours de route)
            if chat_res.tool_calls.is_empty() {
                if policy.mode == GovernanceMode::Constrained && empty_tool_nudges < 3 {
                    empty_tool_nudges += 1;
                    println!("  🔄 [ANTI-STALL] Texte seul en Mode 2 ({}/3 consécutifs), relance automatique...", empty_tool_nudges);
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: "Continue : si la tâche n'est pas encore terminée et validée, appelle immédiatement le prochain outil nécessaire (par ex. `write_code` avec `stage_only: true` pour stager un fichier à la fois, `stage_only: false` pour lancer la validation finale, ou `read_file`/`grep_search`). Si la tâche est vraiment terminée et validée, donne ta réponse finale.".to_string(),
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

            // Exécution des outils demandés (parallèle si plusieurs appels read-only non-cachés)
            let all_readonly_batch = chat_res.tool_calls.len() > 1
                && chat_res.tool_calls.iter().all(|c| matches!(c.name.as_str(), "read_file" | "list_files" | "grep_search"));

            let batch_results: Vec<(String, Option<String>)> = if all_readonly_batch {
                let mut results: Vec<Option<(String, Option<String>)>> = vec![None; chat_res.tool_calls.len()];
                let mut misses: Vec<(usize, &str, &Value)> = Vec::new();
                for (idx, call) in chat_res.tool_calls.iter().enumerate() {
                    let args_key = call.arguments.to_string();
                    if let Some(cached) = tool_cache.get(&call.name, &args_key) {
                        println!("  ♻️ [TOOL CACHE] HIT {} {}", call.name, args_key);
                        results[idx] = Some((cached.to_string(), None));
                    } else {
                        misses.push((idx, call.name.as_str(), &call.arguments));
                    }
                }
                if !misses.is_empty() {
                    let computed: Vec<(usize, (String, Option<String>))> = std::thread::scope(|s| {
                        let handles: Vec<_> = misses.into_iter().map(|(idx, name, args)| {
                            s.spawn(move || {
                                let out = match name {
                                    "read_file" => {
                                        let path = args["path"].as_str().unwrap_or("");
                                        let start_line = args["start_line"].as_u64().map(|v| v as usize);
                                        let end_line = args["end_line"].as_u64().map(|v| v as usize);
                                        match read_file(path, start_line, end_line) {
                                            Ok(res) => res.to_string(),
                                            Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                                        }
                                    }
                                    "grep_search" => {
                                        let query = args["query"].as_str().unwrap_or("");
                                        let root = args["root_path"].as_str().unwrap_or(".");
                                        let max = args["max_results"].as_u64().map(|v| v as usize);
                                        match grep_search(root, query, max) {
                                            Ok(res) => res.to_string(),
                                            Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                                        }
                                    }
                                    "list_files" => {
                                        let root = args["root_path"].as_str().unwrap_or(".");
                                        let depth = args["max_depth"].as_u64().map(|v| v as usize);
                                        match list_files(root, depth) {
                                            Ok(res) => res.to_string(),
                                            Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                                        }
                                    }
                                    _ => String::new(),
                                };
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
                let args_key = normalize_cache_args_key(&self.workspace_root, fn_args);
                let sig = format!("{}:{}", fn_name, args_key);
                if last_tool_sig.as_deref() == Some(&sig) {
                    repeat_count += 1;
                } else {
                    last_tool_sig = Some(sig);
                    repeat_count = 1;
                }

                let is_cacheable_read = matches!(fn_name.as_str(), "read_file" | "list_files" | "grep_search");

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

                let mut tool_output = context_trimmer::truncate_tool_output(&raw_output, 16_000);
                if repeat_count >= 3 {
                    tool_output.push_str("\n\n[ANTI-LOOP] Attention : tu viens de répéter exactement le même appel 3 fois de suite. Change d'approche, passe à l'étape suivante ou conclus.");
                    repeat_count = 0;
                }

                if is_cacheable_read && interrupt_action.is_none() {
                    let path_hint = match fn_name.as_str() {
                        "read_file" => fn_args["path"].as_str(),
                        "list_files" | "grep_search" => fn_args["root_path"].as_str().or(Some(".")),
                        _ => None,
                    };
                    tool_cache.insert(fn_name, &args_key, tool_output.clone(), path_hint);
                } else if matches!(fn_name.as_str(), "write_code" | "edit_file" | "write_file") {
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

                self.record_trace(&run_id, step_order, "tool_output", Some(fn_name), &tool_output);
                on_trace(&run_id, step_order, "tool_output", Some(fn_name), &tool_output, &active_backend_name);
                step_order += 1;

                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: format!("Résultat de l'outil '{}' : {}", fn_name, tool_output),
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
                    self.conn.execute(
                        "UPDATE runs SET status = 'awaiting_approval', completed_at = CURRENT_TIMESTAMP, duration_ms = ?1, total_tokens = ?2 WHERE id = ?3",
                        params![duration_ms as i64, total_tokens as i64, run_id],
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

    fn create_checkpoint(&self) {
        if !self.workspace_root.join(".git").exists() {
            return;
        }
        if let Ok(out) = std::process::Command::new("git")
            .args(["stash", "create", "earnest-checkpoint"])
            .current_dir(&self.workspace_root)
            .output()
        {
            let hash = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let target = if hash.is_empty() { "HEAD" } else { hash.as_str() };
            let _ = std::process::Command::new("git")
                .args(["tag", "-f", "earnest-checkpoint-latest", target])
                .current_dir(&self.workspace_root)
                .output();
            println!("  🔖 [CHECKPOINT] Sauvegardé ({})", target);
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
                std::process::Command::new("cmd").args(["/C", cmd]).current_dir(&self.workspace_root).status()
            } else {
                std::process::Command::new("sh").args(["-c", cmd]).current_dir(&self.workspace_root).status()
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

        match name {
            "read_file" => {
                let path = args["path"].as_str().unwrap_or("");
                let start_line = args["start_line"].as_u64().map(|v| v as usize);
                let end_line = args["end_line"].as_u64().map(|v| v as usize);
                match read_file(path, start_line, end_line) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "grep_search" => {
                let query = args["query"].as_str().unwrap_or("");
                let root = args["root_path"].as_str().unwrap_or(".");
                let max = args["max_results"].as_u64().map(|v| v as usize);
                match grep_search(root, query, max) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "list_files" => {
                let root = args["root_path"].as_str().unwrap_or(".");
                let depth = args["max_depth"].as_u64().map(|v| v as usize);
                match list_files(root, depth) {
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
                n != "invoke_subagent" && (!self.readonly || (n != "edit_file" && n != "write_file" && n != "terminal_exec"))
            })
            .collect();

        let mut final_report = String::new();

        for _ in 0..self.max_steps {
            let (chat_res, _) = self.router.chat_completion_with_fallback(&messages, Some(&sub_tools))?;

            if chat_res.tool_calls.is_empty() {
                final_report = chat_res.content;
                break;
            }

            for call in chat_res.tool_calls {
                let fn_name = &call.name;
                let fn_args = &call.arguments;

                let output = self.dispatch_sub_tool(fn_name, fn_args);
                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: format!("Résultat de l'outil '{}' : {}", fn_name, output),
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

/// Compresse les anciens `tool_output` (> 6 derniers messages) pour garder le KV-cache léger,
/// et compresse davantage si le contexte total dépasse `max_chars`.
fn compact_messages_if_needed(messages: &mut Vec<ChatMessage>, max_chars: usize) {
    let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();
    let soft_limit = (max_chars / 2).max(60_000);
    let keep_tail = 24;
    if total_chars > soft_limit && messages.len() > keep_tail + 2 {
        let split_idx = messages.len().saturating_sub(keep_tail);
        for msg in messages.iter_mut().take(split_idx).skip(2) {
            if msg.role == "user" && msg.content.starts_with("Résultat de l'outil ") && msg.content.len() > 800 {
                let preview: String = msg.content.chars().take(450).collect();
                msg.content = format!(
                    "{}\n... [Sortie outil archivée : {} caractères]",
                    preview,
                    msg.content.len()
                );
            }
        }
    }

    let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();
    if total_chars <= max_chars || messages.len() <= 8 {
        return;
    }
    let keep_tail = 10;
    let split_idx = messages.len().saturating_sub(keep_tail);
    for msg in messages.iter_mut().take(split_idx).skip(2) {
        if msg.content.len() > 600 {
            let preview: String = msg.content.chars().take(450).collect();
            msg.content = format!(
                "{}\n... [Contexte compressé : {} caractères omis]",
                preview,
                msg.content.len().saturating_sub(450)
            );
        }
    }
}
