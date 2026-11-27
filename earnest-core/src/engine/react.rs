use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;

use crate::db::memories::MemoryService;
use crate::llm::client::ChatMessage;
use crate::llm::router::LLMRouter;
use crate::tools::terminal::execute_terminal;
use crate::tools::artifacts::create_artifact;
use crate::tools::web::{search_web, fetch_web_page};
use crate::tools::get_primitive_tools_schemas;

#[derive(Debug, Clone, Serialize)]
pub struct LiveLogEntry {
    pub id: u64,
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

static LIVE_LOGS: OnceLock<Mutex<(u64, VecDeque<LiveLogEntry>)>> = OnceLock::new();

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
        guard.1.iter().filter(|e| e.id > since_id).cloned().collect()
    } else {
        Vec::new()
    }
}

pub fn clear_live_logs() {
    let lock = LIVE_LOGS.get_or_init(|| Mutex::new((0, VecDeque::with_capacity(512))));
    if let Ok(mut guard) = lock.lock() {
        guard.1.clear();
    }
}

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
    pub max_steps: usize,
    pub think_effort: String,
    pub output_dir: Option<String>,
    pub paranoid_mode: bool,
    pub goal_mode: bool,
}

impl<'a> ReActEngine<'a> {
    pub fn new(conn: &'a Connection, router: &'a LLMRouter, project_id: &str) -> Self {
        Self {
            conn,
            router,
            project_id: project_id.to_string(),
            max_steps: 0, // 0 = illimité (protégé par anti-loop et compaction)
            think_effort: "off".to_string(),
            output_dir: None,
            paranoid_mode: false,
            goal_mode: false,
        }
    }

    pub fn resolve_output_dir(custom: Option<&str>) -> std::path::PathBuf {
        let dir = if let Some(s) = custom.map(str::trim).filter(|s| !s.is_empty()) {
            std::path::PathBuf::from(s)
        } else if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
            std::path::PathBuf::from(home).join("Documents").join("Earnest")
        } else {
            std::path::PathBuf::from("Earnest")
        };
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn ensure_checkpoint_table(&self) {
        let _ = self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS run_checkpoints (
                run_id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                prompt TEXT NOT NULL,
                messages_json TEXT NOT NULL,
                step_order INTEGER NOT NULL,
                total_tokens INTEGER NOT NULL,
                updated_at TEXT NOT NULL
            );",
        );
    }

    fn save_checkpoint(&self, run_id: &str, prompt: &str, messages: &[ChatMessage], step_order: usize, total_tokens: usize) {
        self.ensure_checkpoint_table();
        if let Ok(json) = serde_json::to_string(messages) {
            let _ = self.conn.execute(
                "INSERT INTO run_checkpoints (run_id, project_id, prompt, messages_json, step_order, total_tokens, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, CURRENT_TIMESTAMP)
                 ON CONFLICT(run_id) DO UPDATE SET
                   messages_json = excluded.messages_json,
                   step_order = excluded.step_order,
                   total_tokens = excluded.total_tokens,
                   updated_at = CURRENT_TIMESTAMP",
                params![run_id, self.project_id, prompt, json, step_order as i64, total_tokens as i64],
            );
        }
    }

    fn clear_checkpoint(&self, run_id: &str) {
        let _ = self.conn.execute("DELETE FROM run_checkpoints WHERE run_id = ?1", params![run_id]);
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

    fn dispatch_tool(&self, name: &str, args: &Value, run_id: &str) -> String {
        if self.paranoid_mode
            && matches!(
                name,
                "terminal_exec"
                    | "read_file"
                    | "edit_file"
                    | "write_file"
                    | "list_dir"
                    | "list_files"
                    | "grep_search"
                    | "symbol_search"
                    | "workspace_map"
                    | "check_js_syntax"
            )
        {
            return serde_json::json!({
                "status": "blocked",
                "error": "Mode Paranoïaque actif : aucun accès au disque ou au terminal n'est autorisé."
            })
            .to_string();
        }
        match name {
            "check_js_syntax" => {
                let path = args["path"].as_str().unwrap_or("");
                js_syntax_check::check_file(std::path::Path::new(path)).to_json()
            }
            "web_search" => {
                let query = args["query"].as_str().unwrap_or("");
                let max = args["max_results"].as_u64().unwrap_or(5) as usize;
                match search_web(query, max) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "web_fetch" => {
                let url = args["url"].as_str().unwrap_or("");
                let max = args["max_chars"].as_u64().unwrap_or(6000) as usize;
                match fetch_web_page(url, max) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "terminal_exec" => {
                let cmd = args["command"].as_str().unwrap_or("");
                let timeout = args["timeout_sec"].as_u64().unwrap_or(30);
                match execute_terminal(cmd, timeout) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "read_file" => {
                let path = args["path"].as_str().or_else(|| args["file_path"].as_str()).unwrap_or("");
                let start = args["start_line"].as_u64().map(|n| n as usize);
                let end = args["end_line"].as_u64().map(|n| n as usize);
                match crate::tools::fs::read_file_tool(path, start, end) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "edit_file" => {
                let path = args["path"].as_str().or_else(|| args["file_path"].as_str()).unwrap_or("");
                let target = args["target"].as_str().unwrap_or("");
                let replacement = args["replacement"].as_str().unwrap_or("");
                match crate::tools::fs::edit_file_tool(path, target, replacement) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "list_dir" => {
                let path = args["path"].as_str().unwrap_or(".");
                let max_entries = args["max_entries"].as_u64().unwrap_or(80) as usize;
                match crate::tools::fs::list_dir_tool(path, max_entries) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "grep_search" => {
                let query = args["query"].as_str().unwrap_or("");
                let path = args["path"].as_str().unwrap_or(".");
                let max_results = args["max_results"].as_u64().unwrap_or(30) as usize;
                let case_sensitive = args["case_sensitive"].as_bool().unwrap_or(false);
                match crate::tools::fs::grep_search_tool(query, path, max_results, case_sensitive) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "symbol_search" => {
                let query = args["query"].as_str().unwrap_or("");
                let root_str = args["path"].as_str()
                    .or_else(|| args["root_path"].as_str())
                    .or_else(|| args["file_path"].as_str())
                    .or_else(|| args["target_file"].as_str())
                    .unwrap_or(".");
                let root = std::path::Path::new(root_str);
                if tool_guard::path::is_system_sensitive(root) {
                    let err = tool_guard::error::GuardError::SystemPath { requested: root_str.to_string() };
                    return serde_json::json!({"status": "error", "error": err.to_string()}).to_string();
                }
                let limit = args["limit"].as_u64().unwrap_or(8).clamp(1, 25) as usize;
                let include_snippet = args["include_snippet"].as_bool().unwrap_or(true);
                match symbol_index::SymbolIndex::build(root) {
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
                        serde_json::json!({
                            "status": "ok",
                            "total_indexed": idx.len(),
                            "matches": hits,
                        })
                        .to_string()
                    }
                    Err(e) => serde_json::json!({"status": "error", "error": e.to_string()}).to_string(),
                }
            }
            "workspace_map" => {
                let root_str = args["path"].as_str()
                    .or_else(|| args["root_path"].as_str())
                    .or_else(|| args["file_path"].as_str())
                    .or_else(|| args["target_file"].as_str())
                    .unwrap_or(".");
                let root = std::path::Path::new(root_str);
                if tool_guard::path::is_system_sensitive(root) {
                    let err = tool_guard::error::GuardError::SystemPath { requested: root_str.to_string() };
                    return serde_json::json!({"status": "error", "error": err.to_string()}).to_string();
                }
                let max_files = args["max_files"].as_u64().unwrap_or(25).clamp(1, 100) as usize;
                let max_symbols = args["max_symbols_per_file"].as_u64().unwrap_or(8).clamp(1, 30) as usize;
                match symbol_index::workspace_map(root, max_files, max_symbols) {
                    Ok(map) => serde_json::json!({
                        "status": "ok",
                        "map": map,
                    })
                    .to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e.to_string()}).to_string(),
                }
            }
            "create_artifact" => {
                let title = args["title"].as_str().unwrap_or("livrable");
                let content = args["content"].as_str().unwrap_or("");
                let a_type = args["artifact_type"].as_str().unwrap_or("markdown");
                let file_path = args["file_path"].as_str();
                match create_artifact(&self.project_id, title, content, a_type, file_path) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "store_memory" => {
                let key = args["key"].as_str().unwrap_or("");
                let content = args["content"].as_str().unwrap_or("");
                let scope = args["scope"].as_str().unwrap_or("project");
                let mem = MemoryService::new(self.conn);
                match mem.store_memory(&self.project_id, key, content, scope, Some(run_id)) {
                    Ok(msg) => serde_json::json!({"status": "ok", "message": msg}).to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e.to_string()}).to_string(),
                }
            }
            "recall_memory" => {
                let key = args["key"].as_str().unwrap_or("");
                let mem = MemoryService::new(self.conn);
                match mem.recall_memory(&self.project_id, key, Some(run_id)) {
                    Ok(val) => serde_json::json!({"key": key, "found": val.is_some(), "content": val}).to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e.to_string()}).to_string(),
                }
            }
            "list_skills" => crate::tools::skills::list_skills_tool().to_string(),
            "read_skill" => {
                let name = args["name"].as_str().unwrap_or("");
                match crate::tools::skills::read_skill_tool(name) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            "kb_list_products" => call_kb_mcp_tool("kb_list_products", &serde_json::json!({})),
            "kb_search" => {
                let pid = args["product_id"].as_str().unwrap_or("earnest");
                let q = args["query"].as_str().unwrap_or("");
                call_kb_mcp_tool("kb_search", &serde_json::json!({ "product_id": pid, "query": q }))
            }
            "kb_propose" => {
                let pid = args["product_id"].as_str().unwrap_or("earnest");
                let subject_key = args["subject_key"].as_str().unwrap_or("");
                let kind = args["kind"].as_str().unwrap_or("piege");
                let title = args["title"].as_str().unwrap_or("");
                let correction = args["correction"].as_str().unwrap_or("");
                let db_path = crate::tools::resolve_kb_db_path();
                if let Ok(conn) = rusqlite::Connection::open(&db_path) {
                    let _ = conn.execute(
                        "INSERT OR IGNORE INTO products (product_id, name, current_version) VALUES (?, ?, '1.0.0')",
                        rusqlite::params![pid, pid],
                    );
                }
                let mut propose_args = serde_json::json!({
                    "product_id": pid,
                    "subject_key": subject_key,
                    "kind": kind,
                    "source": "agent_discovery",
                    "title": title,
                    "correction": correction
                });
                if let Some(s) = args["symptom"].as_str() { propose_args["symptom"] = serde_json::json!(s); }
                if let Some(c) = args["cause"].as_str() { propose_args["cause"] = serde_json::json!(c); }
                if let Some(e) = args["example"].as_str() { propose_args["example"] = serde_json::json!(e); }
                call_kb_mcp_tool("kb_propose", &propose_args)
            }
            n if n.starts_with("mcp__") => {
                match crate::mcp::client::call_prefixed_mcp_tool(self.conn, n, args) {
                    Ok(res) => res.to_string(),
                    Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
                }
            }
            _ => serde_json::json!({"status": "error", "error": format!("Outil inconnu : {}", name)}).to_string(),
        }
    }

    pub fn run_with_callback<F>(&self, prompt: &str, mut on_trace: F) -> Result<ReActResult, String>
    where
        F: FnMut(&str, usize, &str, Option<&str>, &str, &str),
    {
        let start = Instant::now();
        let run_id = format!("run_{}", &uuid::Uuid::new_v4().to_string().replace('-', "")[..12]);

        // Initialisation du run
        push_live_log("INFO", &format!("🚀 Run {} démarré (projet={}, effort={})", run_id, self.project_id, self.think_effort));
        self.conn.execute(
            "INSERT INTO runs (id, project_id, prompt, provider, model, status, created_at) VALUES (?1, ?2, ?3, 'rust-native', 'rust-react-engine', 'running', CURRENT_TIMESTAMP)",
            params![run_id, self.project_id, prompt],
        ).map_err(|e| e.to_string())?;

        let env_context = detect_runtime_environment();
        let output_dir = Self::resolve_output_dir(self.output_dir.as_deref());
        let skills_summary = crate::tools::skills::skills_prompt_summary();
        let earnest_md = crate::tools::skills::load_earnest_md(std::path::Path::new("."));
        let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M (%:z)").to_string();
        let goal_note = if self.goal_mode {
            " MODE GOAL ACTIF : ne t'arrête jamais sur une réponse partielle. Découpe l'objectif en jalons, vérifie chaque livrable sur disque ou par tests, et termine uniquement quand 100 % de l'objectif est atteint."
        } else {
            ""
        };
        let system_prompt = format!(
            "Tu es Earnest, un système agentique autonome propulsé par un moteur Rust natif ultra-robuste. \
Date et heure actuelles : {}. \
Tu résous la tâche pas à pas (ReAct). Tu disposes d'un accès complet à Internet et au web via 'web_search' et 'web_fetch'. \
Utilise également les autres outils mis à disposition (read_file, edit_file, list_dir, grep_search, workspace_map, symbol_search, terminal_exec, create_artifact, check_js_syntax, store_memory, recall_memory, list_skills, read_skill) quand nécessaire. \
{} \
Environnement réel capturé : {}. \
Dossier de sortie des livrables : {}. Sauf indication contraire explicite dans la consigne, écris tous les fichiers livrables et scripts générés dans ce dossier (jamais dans target\\release ou un dossier système). \
Adapte tes commandes au shell et à l'OS indiqués ci-dessus. \
En cours de mission, n'émets pas de phrase d'annonce sans appel d'outil : appelle directement l'outil suivant. \
Quand ta mission est entièrement accomplie et vérifiée sur disque, fournis directement ta réponse finale complète sans appel d'outil.{}{}",
            now_str,
            skills_summary,
            env_context,
            output_dir.display(),
            goal_note,
            earnest_md
        );

        let mut messages = vec![
            ChatMessage { role: "system".to_string(), content: system_prompt.to_string(), images: None },
            ChatMessage { role: "user".to_string(), content: prompt.to_string(), images: None },
        ];

        let mut tools = get_primitive_tools_schemas();
        let all_mcp = crate::mcp::client::discover_all_mcp_tools(self.conn);
        for mcp_tool in filter_mcp_tools_for_prompt(prompt, &all_mcp) {
            tools.push(crate::mcp::client::to_openai_tool_schema(&mcp_tool));
        }
        let mut step_order = 1;
        let mut total_tokens = 0;
        let mut final_answer = String::new();
        let mut active_backend_name = String::new();
        static GLOBAL_TOOL_CACHE: OnceLock<Mutex<tool_output_cache::ToolCache>> = OnceLock::new();
        let cache_mutex = GLOBAL_TOOL_CACHE.get_or_init(|| Mutex::new(tool_output_cache::ToolCache::new(128)));
        let mut llm_budget = retry_budget::RetryBudget::new(10, 5);

        let mut step_budget_chars = self.router.backends.first()
            .map(|b| self.router.estimate_context_chars(&b.id))
            .unwrap_or(60_000);
        let mut last_backend_id = String::new();
        let mut empty_tool_nudges: usize = 0;
        let mut goal_verifications: usize = 0;
        let mut last_tool_sig = String::new();
        let mut repeat_tool_count: usize = 0;
        let mut loop_detector = loop_detector::LoopDetector::new(
            loop_detector::Config::new(24, 3, 5, 2).unwrap_or_default(),
        );
        let mut step_idx = 0usize;
        let mut tool_history: Vec<(String, String)> = Vec::new();
        crate::llm::client::reset_cancel();
        loop {
            if crate::llm::client::is_cancelled() {
                push_live_log("WARN", "🛑 Run interrompu par l'utilisateur.");
                self.save_checkpoint(&run_id, prompt, &messages, step_order, total_tokens);
                final_answer = "⚠️ Run interrompu par l'utilisateur (checkpoint sauvegardé).".to_string();
                break;
            }
            if step_idx > 0 && step_idx % 5 == 0 {
                self.save_checkpoint(&run_id, prompt, &messages, step_order, total_tokens);
            }
            if self.max_steps > 0 && step_idx >= self.max_steps {
                break;
            }
            step_idx += 1;
            compact_messages(&mut messages, step_budget_chars);
            compact_messages_by_tokens(&mut messages, step_budget_chars);
            if !llm_budget.is_retry_allowed() {
                return Err("Circuit breaker LLM ouvert : trop d'échecs consécutifs dans la fenêtre glissante.".to_string());
            }
            let (chat_res, active_backend) = match self.router.chat_completion_with_fallback(&messages, Some(&tools), &self.think_effort) {
                Ok(res) => {
                    llm_budget.record_success();
                    res
                }
                Err(e) => {
                    llm_budget.record_failure();
                    return Err(e);
                }
            };
            if active_backend != last_backend_id {
                step_budget_chars = self.router.estimate_context_chars(&active_backend);
                last_backend_id = active_backend.clone();
            }
            active_backend_name = active_backend;
            total_tokens += chat_res.total_tokens;

            if let Some(ref reasoning) = chat_res.reasoning_content {
                if !reasoning.trim().is_empty() {
                    push_live_log("THOUGHT", &format!("[{}] {}", active_backend_name, context_trimmer::truncate_tool_output(reasoning, 300)));
                    self.record_trace(&run_id, step_order, "thought", None, reasoning);
                    on_trace(&run_id, step_order, "thought", None, reasoning, &active_backend_name);
                    step_order += 1;
                }
            }

            let is_final_answer = chat_res.tool_calls.is_empty()
                && !(empty_tool_nudges < 5 && looks_like_unfinished_thought(&chat_res.content));

            if !chat_res.content.is_empty() {
                if !is_final_answer {
                    push_live_log("THOUGHT", &format!("[{}] {}", active_backend_name, context_trimmer::truncate_tool_output(&chat_res.content, 300)));
                    self.record_trace(&run_id, step_order, "thought", None, &chat_res.content);
                    on_trace(&run_id, step_order, "thought", None, &chat_res.content, &active_backend_name);
                    step_order += 1;
                }
                messages.push(ChatMessage { role: "assistant".to_string(), content: chat_res.content.clone(), images: None });
            }

            // S'il n'y a aucun appel d'outil, vérifier si le modèle s'est juste arrêté sur une annonce intermédiaire (tolérance 5 tours)
            if chat_res.tool_calls.is_empty() {
                if empty_tool_nudges < 5 && looks_like_unfinished_thought(&chat_res.content) {
                    empty_tool_nudges += 1;
                    push_live_log("WARN", &format!("🔄 [ANTI-STALL] Annonce/réflexion sans outil ({}/5), relance automatique", empty_tool_nudges));
                    eprintln!("  🔄 [ANTI-STALL] Annonce/réflexion sans outil ({}/5), relance automatique...", empty_tool_nudges);
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: "Continue : n'annonce pas ton action en texte seul, appelle immédiatement l'outil nécessaire (ou fournis ta réponse finale complète si toute la mission est terminée et vérifiée).".to_string(),
                        images: None,
                    });
                    continue;
                }
                if self.goal_mode && goal_verifications < 2 {
                    goal_verifications += 1;
                    push_live_log("GOAL", &format!("🎯 [GOAL GATE {}/2] Vérification d'achèvement...", goal_verifications));
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: "GOAL GATE : Avant de conclure, vérifie factuellement (sur disque ou via tests/outils) que 100 % de l'objectif initial est accompli et complet. S'il manque quoi que ce soit, corrige-le immédiatement avec tes outils ; sinon, fournis ta réponse finale définitive.".to_string(),
                        images: None,
                    });
                    continue;
                }
                final_answer = chat_res.content;
                push_live_log("ANSWER", &format!("✅ Run {} terminé ({} tokens)", run_id, total_tokens));
                self.record_trace(&run_id, step_order, "answer", None, &final_answer);
                on_trace(&run_id, step_order, "answer", None, &final_answer, &active_backend_name);
                break;
            }
            empty_tool_nudges = 0;

            // Exécution des outils demandés (dédupliqués sur le tour)
            let mut seen_calls = std::collections::HashSet::new();
            for call in chat_res.tool_calls {
                if !seen_calls.insert((call.name.clone(), call.arguments.to_string())) {
                    continue;
                }
                let fn_name = &call.name;
                let fn_args = &call.arguments;
                tool_history.push((fn_name.clone(), fn_args.to_string()));

                self.record_trace(&run_id, step_order, "tool_call", Some(fn_name), &fn_args.to_string());
                on_trace(&run_id, step_order, "tool_call", Some(fn_name), &fn_args.to_string(), &active_backend_name);
                step_order += 1;

                push_live_log("TOOL", &format!("⚡ {} {}", fn_name, context_trimmer::truncate_tool_output(&fn_args.to_string(), 220)));
                eprintln!("  ⚡ [OUTIL RUST] {} {:?}", fn_name, fn_args);
                let args_key = fn_args.to_string();
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash(&args_key, &mut hasher);
                let args_hash = std::hash::Hasher::finish(&hasher);
                loop_detector.observe(fn_name, args_hash);
                let sig = format!("{}:{}", fn_name, args_key);
                if sig == last_tool_sig {
                    repeat_tool_count += 1;
                } else {
                    last_tool_sig = sig;
                    repeat_tool_count = 1;
                }
                let is_cacheable_read = matches!(fn_name.as_str(), "recall_memory" | "read_file" | "read_skill" | "list_skills" | "list_dir" | "list_files" | "grep_search" | "web_fetch" | "symbol_search" | "workspace_map");
                let raw_output = if is_cacheable_read {
                    let hit = cache_mutex.lock().ok().and_then(|mut c| c.get(fn_name, &args_key).map(|s| s.to_string()));
                    if let Some(cached) = hit {
                        push_live_log("CACHE", &format!("♻️ HIT {}", fn_name));
                        eprintln!("  ♻️ [TOOL CACHE] HIT {} {}", fn_name, args_key);
                        cached
                    } else {
                        self.dispatch_tool(fn_name, fn_args, &run_id)
                    }
                } else {
                    self.dispatch_tool(fn_name, fn_args, &run_id)
                };
                let sanitized_output = log_sanitizer::sanitize_text(&raw_output);
                let tool_output = context_trimmer::truncate_tool_output(&sanitized_output, 16_000);
                push_live_log("OUTPUT", &format!("← {} : {}", fn_name, context_trimmer::truncate_tool_output(&tool_output, 220)));
                let is_tool_error = raw_output.contains("\"status\":\"error\"") || raw_output.contains("\"status\": \"error\"");
                if is_cacheable_read && !is_tool_error {
                    let path_hint = fn_args["path"].as_str().or_else(|| fn_args["key"].as_str());
                    if let Ok(mut c) = cache_mutex.lock() {
                        c.insert(fn_name, &args_key, tool_output.clone(), path_hint);
                    }
                } else if matches!(fn_name.as_str(), "store_memory" | "create_artifact" | "edit_file" | "sandbox_exec" | "terminal_exec" | "execute_terminal") {
                    loop_detector.reset_progress();
                    if let Some(hint) = fn_args["path"].as_str().or_else(|| fn_args["key"].as_str()) {
                        if let Ok(mut c) = cache_mutex.lock() {
                            c.invalidate_path(hint);
                        }
                    }
                }

                self.record_trace(&run_id, step_order, "tool_output", Some(fn_name), &tool_output);
                on_trace(&run_id, step_order, "tool_output", Some(fn_name), &tool_output, &active_backend_name);
                step_order += 1;

                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: format_tool_observation(fn_name, fn_args, &tool_output),
                    images: None,
                });
            }

            if repeat_tool_count >= 6 {
                push_live_log("WARN", "🛑 [ANTI-LOOP] Même appel d'outil répété 6 fois de suite, arrêt de sécurité.");
                final_answer = "⚠️ Arrêt de sécurité [ANTI-LOOP] : le modèle répète exactement le même appel d'outil en boucle.".to_string();
                break;
            } else if repeat_tool_count == 4 {
                push_live_log("WARN", "🔁 [ANTI-LOOP] Répétition détectée (4x), rappel envoyé au modèle.");
                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: "Attention [ANTI-LOOP] : tu viens d'appeler exactement le même outil avec les mêmes arguments 4 fois de suite. Change d'approche ou conclus ta mission.".to_string(),
                    images: None,
                });
            } else if let loop_detector::Verdict::LoopDetected { rule, confidence } = loop_detector.evaluate() {
                push_live_log("WARN", &format!("🔁 [LOOP-DETECTOR] Règle '{}' déclenchée (score {}).", rule, confidence));
                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: format!("Attention [ANTI-LOOP:{}] : motif répétitif ou cyclique détecté (score {}). Change de stratégie ou conclus.", rule, confidence),
                    images: None,
                });
                loop_detector.reset_progress();
            }
        }

        if final_answer.is_empty() && self.max_steps > 0 {
            let mut synth_msgs = messages.clone();
            synth_msgs.push(ChatMessage {
                role: "user".to_string(),
                content: "Tu as atteint la limite d'étapes. Synthétise immédiatement ta réponse finale complète en français à partir des informations déjà collectées, sans appeler d'outil.".to_string(),
                images: None,
            });
            if let Ok((synth_res, _)) = self.router.chat_completion_with_fallback(&synth_msgs, None, &self.think_effort) {
                if !synth_res.content.trim().is_empty() {
                    final_answer = synth_res.content;
                    total_tokens += synth_res.total_tokens;
                }
            }
            if final_answer.is_empty() {
                final_answer = format!("⚠️ Limite de {} tours atteinte sans réponse finale.", self.max_steps);
            }
        }

        let duration_ms = start.elapsed().as_millis();
        if !crate::llm::client::is_cancelled() {
            self.clear_checkpoint(&run_id);
            if let Some(sug) = react_resilience::suggest_wfl_from_tool_trace(&run_id, &tool_history) {
                let dir = std::path::Path::new(".wf").join("suggestions");
                if std::fs::create_dir_all(&dir).is_ok() {
                    let path = dir.join(format!("{}.wf.yaml", run_id));
                    if std::fs::write(&path, &sug.yaml).is_ok() {
                        let note = format!(
                            "💡 Brouillon WFL proposé ({} x{}) → {}",
                            sug.pattern_summary,
                            sug.repetitions,
                            path.display()
                        );
                        push_live_log("INFO", &note);
                        self.record_trace(&run_id, step_order, "wfl_suggestion", None, &note);
                        on_trace(&run_id, step_order, "wfl_suggestion", None, &note, &active_backend_name);
                    }
                }
            }
        }

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

    /// Reprend un run interrompu depuis son dernier checkpoint SQLite (`run_checkpoints`).
    pub fn resume_with_callback<F>(&self, run_id: &str, mut on_trace: F) -> Result<ReActResult, String>
    where
        F: FnMut(&str, usize, &str, Option<&str>, &str, &str),
    {
        self.ensure_checkpoint_table();
        let (prompt, messages_json, mut step_order, mut total_tokens): (String, String, i64, i64) = self
            .conn
            .query_row(
                "SELECT prompt, messages_json, step_order, total_tokens FROM run_checkpoints WHERE run_id = ?1",
                params![run_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(|_| format!("Aucun checkpoint trouvé pour '{}'", run_id))?;

        let mut messages: Vec<ChatMessage> = serde_json::from_str(&messages_json)
            .map_err(|e| format!("Checkpoint corrompu: {}", e))?;
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: "Reprise après interruption : continue exactement à partir de l'état ci-dessus sans refaire ce qui est déjà validé.".to_string(),
            images: None,
        });

        let start = Instant::now();
        let mut tools = get_primitive_tools_schemas();
        let all_mcp = crate::mcp::client::discover_all_mcp_tools(self.conn);
        for mcp_tool in filter_mcp_tools_for_prompt(&prompt, &all_mcp) {
            tools.push(crate::mcp::client::to_openai_tool_schema(&mcp_tool));
        }

        let mut step_budget_chars = self.router.backends.first()
            .map(|b| self.router.estimate_context_chars(&b.id))
            .unwrap_or(60_000);
        let mut empty_tool_nudges = 0usize;
        let mut goal_verifications = 0usize;
        let mut final_answer = String::new();
        let mut step_idx = 0usize;
        crate::llm::client::reset_cancel();

        loop {
            if crate::llm::client::is_cancelled() {
                self.save_checkpoint(run_id, &prompt, &messages, step_order as usize, total_tokens as usize);
                final_answer = "⚠️ Run interrompu (checkpoint mis à jour).".to_string();
                break;
            }
            if self.max_steps > 0 && step_idx >= self.max_steps {
                break;
            }
            step_idx += 1;
            if step_idx % 5 == 0 {
                self.save_checkpoint(run_id, &prompt, &messages, step_order as usize, total_tokens as usize);
            }
            compact_messages(&mut messages, step_budget_chars);
            compact_messages_by_tokens(&mut messages, step_budget_chars);

            let (chat_res, active_backend) = self.router.chat_completion_with_fallback(&messages, Some(&tools), &self.think_effort)?;
            step_budget_chars = self.router.estimate_context_chars(&active_backend);
            total_tokens += chat_res.total_tokens as i64;
            if let Some(ref reasoning) = chat_res.reasoning_content {
                if !reasoning.trim().is_empty() {
                    self.record_trace(run_id, step_order as usize, "thought", None, reasoning);
                    on_trace(run_id, step_order as usize, "thought", None, reasoning, &active_backend);
                    step_order += 1;
                }
            }
            let is_final_answer = chat_res.tool_calls.is_empty()
                && !(empty_tool_nudges < 3 && looks_like_unfinished_thought(&chat_res.content));
            if !chat_res.content.is_empty() {
                if !is_final_answer {
                    self.record_trace(run_id, step_order as usize, "thought", None, &chat_res.content);
                    on_trace(run_id, step_order as usize, "thought", None, &chat_res.content, &active_backend);
                    step_order += 1;
                }
                messages.push(ChatMessage { role: "assistant".to_string(), content: chat_res.content.clone(), images: None });
            }
            if chat_res.tool_calls.is_empty() {
                if empty_tool_nudges < 3 && looks_like_unfinished_thought(&chat_res.content) {
                    empty_tool_nudges += 1;
                    messages.push(ChatMessage { role: "user".to_string(), content: "Continue : appelle immédiatement l'outil suivant.".to_string(), images: None });
                    continue;
                }
                if self.goal_mode && goal_verifications < 2 {
                    goal_verifications += 1;
                    push_live_log("GOAL", &format!("🎯 [GOAL GATE {}/2] Vérification d'achèvement...", goal_verifications));
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: "GOAL GATE : Avant de conclure, vérifie factuellement (sur disque ou via tests/outils) que 100 % de l'objectif initial est accompli et complet. S'il manque quoi que ce soit, corrige-le immédiatement avec tes outils ; sinon, fournis ta réponse finale définitive.".to_string(),
                        images: None,
                    });
                    continue;
                }
                final_answer = chat_res.content;
                self.record_trace(run_id, step_order as usize, "answer", None, &final_answer);
                on_trace(run_id, step_order as usize, "answer", None, &final_answer, &active_backend);
                break;
            }
            empty_tool_nudges = 0;
            let mut seen_calls = std::collections::HashSet::new();
            for call in chat_res.tool_calls {
                if !seen_calls.insert((call.name.clone(), call.arguments.to_string())) {
                    continue;
                }
                let output = self.dispatch_tool(&call.name, &call.arguments, run_id);
                self.record_trace(run_id, step_order as usize, "tool_output", Some(&call.name), &output);
                on_trace(run_id, step_order as usize, "tool_output", Some(&call.name), &output, &active_backend);
                step_order += 1;
                messages.push(ChatMessage { role: "tool".to_string(), content: context_trimmer::truncate_tool_output(&output, 4000), images: None });
            }
        }

        if !crate::llm::client::is_cancelled() {
            self.clear_checkpoint(run_id);
        }
        let duration_ms = start.elapsed().as_millis();
        Ok(ReActResult {
            run_id: run_id.to_string(),
            status: "success".to_string(),
            answer: final_answer,
            steps_count: step_order as usize,
            total_tokens: total_tokens as usize,
            duration_ms,
        })
    }

    pub fn run(&self, prompt: &str) -> Result<ReActResult, String> {
        self.run_with_callback(prompt, |_, _, _, _, _, _| {})
    }

    /// Pipeline Multi-Agents : Architecte (plan) -> Worker (ReAct outils) -> Critique (revue).
    pub fn run_multiagent_with_callback<F>(&self, prompt: &str, mut on_trace: F) -> Result<ReActResult, String>
    where
        F: FnMut(&str, usize, &str, Option<&str>, &str, &str),
    {
        push_live_log("INFO", "🧠 [MULTI-AGENT] Phase 1/3 : Architecte (planification)...");
        let arch_msgs = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "Tu es l'Architecte. Rédige un plan d'action court (4 à 6 étapes numérotées) et les vérifications disque/mémoire obligatoires pour accomplir la mission ci-dessous.".to_string(),
                images: None,
            },
            ChatMessage { role: "user".to_string(), content: prompt.to_string(), images: None },
        ];
        let plan = match self.router.chat_completion_with_fallback(&arch_msgs, None, &self.think_effort) {
            Ok((res, backend)) => {
                on_trace("multiagent", 0, "thought", None, &format!("🧠 [Architecte]\n{}", res.content), &backend);
                push_live_log("THOUGHT", &format!("🧠 [Architecte] {}", context_trimmer::truncate_tool_output(&res.content, 240)));
                res.content
            }
            Err(_) => String::new(),
        };

        let worker_prompt = if plan.is_empty() {
            prompt.to_string()
        } else {
            format!("{}\n\n--- PLAN DE L'ARCHITECTE ---\n{}", prompt, plan)
        };

        push_live_log("INFO", "⚙️ [MULTI-AGENT] Phase 2/3 : Worker (exécution)...");
        let mut result = self.run_with_callback(&worker_prompt, &mut on_trace)?;

        if crate::llm::client::is_cancelled() {
            return Ok(result);
        }

        push_live_log("INFO", "🔍 [MULTI-AGENT] Phase 3/3 : Critique (revue finale)...");
        let critic_msgs = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "Tu es le Critique qualité. Vérifie si la réponse du Worker répond à TOUTES les exigences de la consigne. Réponds par 'VALIDÉ' suivi d'un court verdict, ou par 'INCOMPLET:' suivi des points exacts manquants.".to_string(),
                images: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: format!("Consigne:\n{}\n\nRéponse du Worker:\n{}", prompt, result.answer),
                images: None,
            },
        ];
        if let Ok((crit, backend)) = self.router.chat_completion_with_fallback(&critic_msgs, None, &self.think_effort) {
            on_trace(&result.run_id, result.steps_count + 1, "thought", None, &format!("🔍 [Critique]\n{}", crit.content), &backend);
            push_live_log("THOUGHT", &format!("🔍 [Critique] {}", context_trimmer::truncate_tool_output(&crit.content, 240)));
            result.total_tokens += crit.total_tokens;
            if crit.content.to_uppercase().contains("INCOMPLET:") {
                push_live_log("WARN", "🔧 [MULTI-AGENT] Critique INCOMPLET, passe de correction Worker...");
                let fix_engine = ReActEngine {
                    conn: self.conn,
                    router: self.router,
                    project_id: self.project_id.clone(),
                    max_steps: 6,
                    think_effort: self.think_effort.clone(),
                    output_dir: self.output_dir.clone(),
                    paranoid_mode: self.paranoid_mode,
                    goal_mode: self.goal_mode,
                };
                let fix_prompt = format!("{}\n\n--- RETOUR CRITIQUE À CORRIGER ---\n{}", prompt, crit.content);
                if let Ok(fix_res) = fix_engine.run_with_callback(&fix_prompt, &mut on_trace) {
                    result.answer = fix_res.answer;
                    result.total_tokens += fix_res.total_tokens;
                    result.steps_count += fix_res.steps_count;
                }
            }
        }
        Ok(result)
    }
}

/// Filtre les outils MCP selon l'intention du prompt via `react_resilience`.
fn filter_mcp_tools_for_prompt(
    prompt: &str,
    tools: &[crate::mcp::client::McpToolInfo],
) -> Vec<crate::mcp::client::McpToolInfo> {
    match react_resilience::decide_mcp_servers_for_prompt(prompt, tools.len()) {
        react_resilience::McpFilterDecision::All => tools.to_vec(),
        react_resilience::McpFilterDecision::None => Vec::new(),
        react_resilience::McpFilterDecision::AllowServers(servers) => tools
            .iter()
            .filter(|t| servers.iter().any(|s| *s == t.server_name))
            .cloned()
            .collect(),
    }
}

fn to_resilience_msgs(messages: &[ChatMessage]) -> Vec<react_resilience::CompactionMessage> {
    messages
        .iter()
        .map(|m| react_resilience::CompactionMessage {
            role: m.role.clone(),
            content: m.content.clone(),
            has_images: m.images.is_some(),
        })
        .collect()
}

fn apply_resilience_msgs(messages: &mut Vec<ChatMessage>, compacted: Vec<react_resilience::CompactionMessage>) {
    if compacted.len() == messages.len()
        && compacted.iter().zip(messages.iter()).all(|(a, b)| a.role == b.role && a.content == b.content)
    {
        return;
    }
    *messages = compacted
        .into_iter()
        .map(|m| ChatMessage {
            role: m.role,
            content: m.content,
            images: None,
        })
        .collect();
}

/// Compacte l'historique `messages` avec Rolling Working Memory via `react_resilience`.
fn compact_messages(messages: &mut Vec<ChatMessage>, soft_limit: usize) {
    let mut tmp = to_resilience_msgs(messages);
    react_resilience::compact_with_rolling_memory(&mut tmp, soft_limit);
    apply_resilience_msgs(messages, tmp);
}

/// Filet de sécurité par budget de tokens (`react_resilience` + `token-compactor`).
fn compact_messages_by_tokens(messages: &mut Vec<ChatMessage>, max_chars: usize) {
    let mut tmp = to_resilience_msgs(messages);
    react_resilience::compact_by_tokens(&mut tmp, max_chars);
    apply_resilience_msgs(messages, tmp);
}

/// Détecte si une réponse texte sans `tool_calls` ressemble à une annonce intermédiaire.
fn looks_like_unfinished_thought(content: &str) -> bool {
    react_resilience::looks_like_unfinished_thought(content)
}

/// Capture dynamiquement l'environnement système réel (OS, version, architecture,
/// shell d'exécution de `terminal_exec`, runtimes disponibles et répertoire courant).
/// Les sondes lentes (version OS, runtimes) sont mises en cache via `OnceLock`.
pub fn detect_runtime_environment() -> String {
    static STATIC_ENV: OnceLock<String> = OnceLock::new();
    let base = STATIC_ENV.get_or_init(|| {
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;

        let os_version = if cfg!(target_os = "windows") {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", "ver"]);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                c.creation_flags(0x0800_0000);
            }
            c.output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Windows".to_string())
        } else {
            std::process::Command::new("uname")
                .args(["-sr"])
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| os.to_string())
        };

        let shell = if cfg!(target_os = "windows") {
            "PowerShell (séparateur ';' ; commandes PowerShell/Windows natives)"
        } else {
            "sh/bash (POSIX)"
        };

        let mut runtimes = Vec::new();
        for (bin, arg, label) in [
            ("python", "--version", "Python"),
            ("cargo", "--version", "Cargo"),
            ("git", "--version", "Git"),
        ] {
            let mut c = std::process::Command::new(bin);
            c.arg(arg);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                c.creation_flags(0x0800_0000);
            }
            if let Ok(out) = c.output() {
                if out.status.success() {
                    let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    let first_line = ver.lines().next().unwrap_or(label).trim();
                    runtimes.push(first_line.to_string());
                }
            }
        }
        let rt_str = if runtimes.is_empty() {
            "aucun détecté".to_string()
        } else {
            runtimes.join(", ")
        };

        format!(
            "OS={} ({}, {}), Shell={}, Runtimes=[{}]",
            os, arch, os_version, shell, rt_str
        )
    });

    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string());
    format!("{}, CWD={}", base, cwd)
}

fn call_kb_mcp_tool(raw_name: &str, args: &Value) -> String {
    let kb_exe = crate::tools::resolve_kb_exe_path();
    let kb_db = crate::tools::resolve_kb_db_path();
    let cfg = crate::mcp::client::McpServerConfig {
        name: "bcpe".to_string(),
        command: kb_exe.to_string_lossy().to_string(),
        args: vec![
            "serve".to_string(),
            "--db".to_string(),
            kb_db.to_string_lossy().to_string(),
        ],
    };
    match crate::mcp::client::McpStdioClient::spawn(&cfg) {
        Ok(mut client) => match client.call_tool(raw_name, args) {
            Ok(res) => res.to_string(),
            Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
        },
        Err(e) => serde_json::json!({"status": "error", "error": e}).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_runtime_environment_captures_real_host_info() {
        let env = detect_runtime_environment();
        assert!(env.contains(&format!("OS={}", std::env::consts::OS)));
        assert!(env.contains("Shell="));
        assert!(env.contains("CWD="));
    }

    #[test]
    fn unfinished_thought_detected_for_mid_run_announcements() {
        assert!(looks_like_unfinished_thought(
            "L'environnement d'exécution est PowerShell lui-même. Je vais utiliser le séparateur PowerShell `;`."
        ));
        assert!(looks_like_unfinished_thought("Let me run the script:"));
        assert!(looks_like_unfinished_thought(
            "Le serveur est cohérent : il rejette un kind non admis. Je relance avec un kind valide."
        ));
        assert!(looks_like_unfinished_thought(
            "Simulation réussie (3 transferts simulés). Je scelle maintenant le workflow."
        ));
        assert!(!looks_like_unfinished_thought(
            "Mission terminée : 33 fiches HTML générées dans D:\\Projet\\Hernest\\projects_test\\models_g_html\\index.html."
        ));
    }

    #[test]
    fn compact_messages_preserves_head_and_tail_over_limit() {
        let mut msgs: Vec<ChatMessage> = (0..30)
            .map(|i| ChatMessage {
                role: if i == 0 { "system" } else { "user" }.to_string(),
                content: "x".repeat(3_000),
                images: None,
            })
            .collect();
        compact_messages(&mut msgs, 60_000);
        // 2 head + 1 marker + 16 tail = 19
        assert_eq!(msgs.len(), 19);
        assert!(msgs[2].content.contains("ROLLING WORKING MEMORY"));
    }

    #[test]
    fn filter_mcp_tools_targets_relevant_server_or_broad_prompts() {
        use crate::mcp::client::McpToolInfo;
        let mut tools = Vec::new();
        for i in 0..5 {
            tools.push(McpToolInfo {
                server_name: "wfl".into(),
                raw_name: format!("wfl_{}", i),
                full_name: format!("mcp__wfl__wfl_{}", i),
                description: String::new(),
                input_schema: serde_json::json!({}),
            });
            tools.push(McpToolInfo {
                server_name: "bcpe".into(),
                raw_name: format!("kb_{}", i),
                full_name: format!("mcp__bcpe__kb_{}", i),
                description: String::new(),
                input_schema: serde_json::json!({}),
            });
        }
        let wf_only = filter_mcp_tools_for_prompt("Crée un workflow YAML", &tools);
        assert_eq!(wf_only.len(), 5);
        assert!(wf_only.iter().all(|t| t.server_name == "wfl"));

        let broad = filter_mcp_tools_for_prompt("Fais une auto-évaluation", &tools);
        assert_eq!(broad.len(), 10);

        let none = filter_mcp_tools_for_prompt("Calcule 2+2", &tools);
        assert!(none.is_empty());
    }

    #[test]
    fn checkpoint_roundtrip_saves_and_clears_in_sqlite() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let router = LLMRouter { backends: vec![] };
        let engine = ReActEngine::new(&conn, &router, "proj");
        let msgs = vec![ChatMessage {
            role: "user".into(),
            content: "hello".into(),
            images: None,
        }];
        engine.save_checkpoint("run_chk_1", "hello", &msgs, 4, 120);
        let cnt: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM run_checkpoints WHERE run_id = 'run_chk_1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cnt, 1);
        engine.clear_checkpoint("run_chk_1");
        let cnt_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM run_checkpoints WHERE run_id = 'run_chk_1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cnt_after, 0);
    }

    #[test]
    fn push_live_log_redacts_secrets() {
        push_live_log("INFO", "Authorization: Bearer secret_jwt_token_12345678 sk-proj-abcdef1234567890");
        let logs = get_live_logs(0);
        let last = logs.last().expect("expected a live log entry");
        assert!(!last.message.contains("secret_jwt_token_12345678"));
        assert!(!last.message.contains("sk-proj-abcdef1234567890"));
        assert!(last.message.contains("[REDACTED:BEARER_TOKEN]"));
        assert!(last.message.contains("[REDACTED:API_KEY]"));
    }

    #[test]
    fn symbol_search_tool_finds_earnest_symbols_with_snippet() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let router = LLMRouter::from_db(&conn);
        let engine = ReActEngine {
            conn: &conn,
            router: &router,
            project_id: "test".to_string(),
            max_steps: 2,
            think_effort: "low".to_string(),
            output_dir: None,
            paranoid_mode: false,
            goal_mode: false,
        };
        let args = serde_json::json!({
            "query": "SymbolIndex",
            "path": "../crates/symbol-index",
            "limit": 3,
            "include_snippet": true
        });
        let raw = engine.dispatch_tool("symbol_search", &args, "run_test");
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["status"], "ok");
        let matches = parsed["matches"].as_array().unwrap();
        assert!(!matches.is_empty());
        assert_eq!(matches[0]["name"], "SymbolIndex");
        assert!(matches[0]["snippet"].as_str().unwrap().contains("SymbolIndex"));
    }

    #[test]
    fn workspace_map_tool_returns_compact_map() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let router = LLMRouter::from_db(&conn);
        let engine = ReActEngine {
            conn: &conn,
            router: &router,
            project_id: "test".to_string(),
            max_steps: 2,
            think_effort: "low".to_string(),
            output_dir: None,
            paranoid_mode: false,
            goal_mode: false,
        };
        let args = serde_json::json!({
            "path": "../crates/symbol-index",
            "max_files": 5,
            "max_symbols_per_file": 4
        });
        let raw = engine.dispatch_tool("workspace_map", &args, "run_test");
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["status"], "ok");
        let map = parsed["map"].as_str().unwrap();
        assert!(map.contains("src/indexer.rs:"));
        assert!(map.contains("struct SymbolIndex("));
    }

    #[test]
    fn format_tool_observation_preserves_tool_args_without_imitation() {
        let args = serde_json::json!({
            "path": "src/main.rs",
            "target": "old_fn()",
            "replacement": "new_fn()"
        });
        let obs = super::format_tool_observation("edit_file", &args, "{\"status\":\"ok\"}");
        assert!(obs.contains("Résultat de l'outil 'edit_file'"));
        assert!(obs.contains("src/main.rs"));
        assert!(obs.contains("new_fn()"));
        assert!(!obs.contains("[Appel outil:"));
    }
}

fn format_tool_observation(fn_name: &str, fn_args: &Value, tool_output: &str) -> String {
    let args_preview = context_trimmer::truncate_tool_output(&fn_args.to_string(), 1_200);
    format!(
        "Résultat de l'outil '{}' (arguments soumis: {}) : {}",
        fn_name, args_preview, tool_output
    )
}
