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
}

impl<'a> ReActEngine<'a> {
    pub fn new(conn: &'a Connection, router: &'a LLMRouter, project_id: &str) -> Self {
        Self {
            conn,
            router,
            project_id: project_id.to_string(),
            max_steps: 0, // 0 = illimité (protégé par anti-loop et compaction)
            think_effort: "off".to_string(),
        }
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
        match name {
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
            "symbol_search" => {
                let query = args["query"].as_str().unwrap_or("");
                let root_str = args["path"].as_str().unwrap_or(".");
                let limit = args["limit"].as_u64().unwrap_or(8).clamp(1, 25) as usize;
                let include_snippet = args["include_snippet"].as_bool().unwrap_or(true);
                let root = std::path::Path::new(root_str);
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
                let root_str = args["path"].as_str().unwrap_or(".");
                let max_files = args["max_files"].as_u64().unwrap_or(25).clamp(1, 100) as usize;
                let max_symbols = args["max_symbols_per_file"].as_u64().unwrap_or(8).clamp(1, 30) as usize;
                let root = std::path::Path::new(root_str);
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
        let system_prompt = format!(
            "Tu es Earnest, un système agentique autonome propulsé par un moteur Rust natif ultra-robuste. \
Tu résous la tâche pas à pas (ReAct). Tu disposes d'un accès complet à Internet et au web via 'web_search' et 'web_fetch'. \
Utilise également les autres outils mis à disposition (terminal_exec, create_artifact, store_memory, recall_memory) quand nécessaire. \
Environnement réel capturé : {}. \
Adapte tes commandes au shell et à l'OS indiqués ci-dessus. \
En cours de mission, n'émets pas de phrase d'annonce sans appel d'outil : appelle directement l'outil suivant. \
Quand ta mission est entièrement accomplie et vérifiée sur disque, fournis directement ta réponse finale complète sans appel d'outil.",
            env_context
        );

        let mut messages = vec![
            ChatMessage { role: "system".to_string(), content: system_prompt.to_string(), images: None },
            ChatMessage { role: "user".to_string(), content: prompt.to_string(), images: None },
        ];

        let tools = get_primitive_tools_schemas();
        let mut step_order = 1;
        let mut total_tokens = 0;
        let mut final_answer = String::new();
        let mut active_backend_name: String;
        static GLOBAL_TOOL_CACHE: OnceLock<Mutex<tool_output_cache::ToolCache>> = OnceLock::new();
        let cache_mutex = GLOBAL_TOOL_CACHE.get_or_init(|| Mutex::new(tool_output_cache::ToolCache::new(128)));
        let mut llm_budget = retry_budget::RetryBudget::new(10, 5);

        let mut step_budget_chars = self.router.backends.first()
            .map(|b| self.router.estimate_context_chars(&b.id))
            .unwrap_or(60_000);
        let mut last_backend_id = String::new();
        let mut empty_tool_nudges: usize = 0;
        let mut last_tool_sig = String::new();
        let mut repeat_tool_count: usize = 0;
        let mut step_idx = 0usize;
        crate::llm::client::reset_cancel();
        loop {
            if crate::llm::client::is_cancelled() {
                push_live_log("WARN", "🛑 Run interrompu par l'utilisateur.");
                final_answer = "⚠️ Run interrompu par l'utilisateur.".to_string();
                break;
            }
            if self.max_steps > 0 && step_idx >= self.max_steps {
                break;
            }
            step_idx += 1;
            compact_messages(&mut messages, step_budget_chars);
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

            if !chat_res.content.is_empty() {
                push_live_log("THOUGHT", &format!("[{}] {}", active_backend_name, context_trimmer::truncate_tool_output(&chat_res.content, 300)));
                self.record_trace(&run_id, step_order, "thought", None, &chat_res.content);
                on_trace(&run_id, step_order, "thought", None, &chat_res.content, &active_backend_name);
                step_order += 1;
                messages.push(ChatMessage { role: "assistant".to_string(), content: chat_res.content.clone(), images: None });
            }

            // S'il n'y a aucun appel d'outil, vérifier si le modèle s'est juste arrêté sur une annonce intermédiaire
            if chat_res.tool_calls.is_empty() {
                if empty_tool_nudges < 3 && looks_like_unfinished_thought(&chat_res.content) {
                    empty_tool_nudges += 1;
                    push_live_log("WARN", &format!("🔄 [ANTI-STALL] Annonce sans outil ({}/3), relance automatique", empty_tool_nudges));
                    println!("  🔄 [ANTI-STALL] Annonce sans outil ({}/3), relance automatique...", empty_tool_nudges);
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: "Continue : n'annonce pas ton action en texte seul, appelle immédiatement l'outil nécessaire (ou fournis ta réponse finale complète si toute la mission est terminée et vérifiée).".to_string(),
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

            // Exécution des outils demandés
            for call in chat_res.tool_calls {
                let fn_name = &call.name;
                let fn_args = &call.arguments;

                self.record_trace(&run_id, step_order, "tool_call", Some(fn_name), &fn_args.to_string());
                on_trace(&run_id, step_order, "tool_call", Some(fn_name), &fn_args.to_string(), &active_backend_name);
                step_order += 1;

                push_live_log("TOOL", &format!("⚡ {} {}", fn_name, context_trimmer::truncate_tool_output(&fn_args.to_string(), 220)));
                println!("  ⚡ [OUTIL RUST] {} {:?}", fn_name, fn_args);
                let args_key = fn_args.to_string();
                let sig = format!("{}:{}", fn_name, args_key);
                if sig == last_tool_sig {
                    repeat_tool_count += 1;
                } else {
                    last_tool_sig = sig;
                    repeat_tool_count = 1;
                }
                let is_cacheable_read = matches!(fn_name.as_str(), "recall_memory" | "read_file" | "list_files" | "web_fetch" | "symbol_search" | "workspace_map");
                let raw_output = if is_cacheable_read {
                    let hit = cache_mutex.lock().ok().and_then(|mut c| c.get(fn_name, &args_key).map(|s| s.to_string()));
                    if let Some(cached) = hit {
                        push_live_log("CACHE", &format!("♻️ HIT {}", fn_name));
                        println!("  ♻️ [TOOL CACHE] HIT {} {}", fn_name, args_key);
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
                if is_cacheable_read {
                    let path_hint = fn_args["path"].as_str().or_else(|| fn_args["key"].as_str());
                    if let Ok(mut c) = cache_mutex.lock() {
                        c.insert(fn_name, &args_key, tool_output.clone(), path_hint);
                    }
                } else if matches!(fn_name.as_str(), "store_memory" | "create_artifact" | "sandbox_exec" | "execute_terminal") {
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
                    content: format!("Résultat de l'outil '{}' : {}", fn_name, tool_output),
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
            }
        }

        if final_answer.is_empty() && self.max_steps > 0 {
            final_answer = format!("⚠️ Limite de {} tours atteinte sans réponse finale.", self.max_steps);
        }

        let duration_ms = start.elapsed().as_millis();

        self.conn.execute(
            "UPDATE runs SET status = 'success', completed_at = CURRENT_TIMESTAMP, duration_ms = ?1, total_tokens = ?2 WHERE id = ?3",
            params![duration_ms as i64, total_tokens as i64, run_id],
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

/// Compacte l'historique `messages` s'il dépasse `soft_limit` caractères, en
/// conservant le prompt système, la requête initiale et les derniers échanges.
fn compact_messages(messages: &mut Vec<ChatMessage>, soft_limit: usize) {
    let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();
    if total_chars <= soft_limit || messages.len() <= 18 {
        return;
    }
    let keep_head = 2usize.min(messages.len());
    let keep_tail = 16usize;
    if messages.len() <= keep_head + keep_tail {
        return;
    }
    let drop_end = messages.len() - keep_tail;
    let dropped = drop_end - keep_head;
    let mut compacted = Vec::with_capacity(keep_head + keep_tail + 1);
    compacted.extend_from_slice(&messages[..keep_head]);
    compacted.push(ChatMessage {
        role: "user".to_string(),
        content: format!("[Contexte compacté : {} anciens messages résumés pour préserver la fenêtre LLM]", dropped),
        images: None,
    });
    compacted.extend_from_slice(&messages[drop_end..]);
    *messages = compacted;
}

/// Détecte si une réponse texte sans `tool_calls` ressemble à une annonce
/// intermédiaire (ex: "Je vais exécuter...") plutôt qu'à une conclusion finale.
fn looks_like_unfinished_thought(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.len() >= 500 {
        return false;
    }
    let lower = trimmed.to_lowercase();
    if trimmed.ends_with(':') {
        return true;
    }
    [
        "je vais ",
        "let me ",
        "i will ",
        "i'll ",
        "je lance ",
        "je vérifie",
        "utilisons ",
        "exécutons ",
        "séparateur powershell",
    ]
    .iter()
    .any(|pat| lower.contains(pat))
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
            std::process::Command::new("cmd")
                .args(["/C", "ver"])
                .output()
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
            if let Ok(out) = std::process::Command::new(bin).arg(arg).output() {
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
        assert!(msgs[2].content.contains("Contexte compacté"));
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
}
