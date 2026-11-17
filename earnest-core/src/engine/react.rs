use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use rusqlite::{params, Connection};
use serde_json::Value;

use crate::db::memories::MemoryService;
use crate::llm::client::ChatMessage;
use crate::llm::router::LLMRouter;
use crate::tools::terminal::execute_terminal;
use crate::tools::artifacts::create_artifact;
use crate::tools::web::{search_web, fetch_web_page};
use crate::tools::get_primitive_tools_schemas;

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
}

impl<'a> ReActEngine<'a> {
    pub fn new(conn: &'a Connection, router: &'a LLMRouter, project_id: &str) -> Self {
        Self {
            conn,
            router,
            project_id: project_id.to_string(),
            max_steps: 24,
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
        self.conn.execute(
            "INSERT INTO runs (id, project_id, prompt, provider, model, status, created_at) VALUES (?1, ?2, ?3, 'rust-native', 'rust-react-engine', 'running', CURRENT_TIMESTAMP)",
            params![run_id, self.project_id, prompt],
        ).map_err(|e| e.to_string())?;

        let system_prompt = "Tu es Earnest, un système agentique autonome propulsé par un moteur Rust natif ultra-robuste. \
Tu résous la tâche pas à pas (ReAct). Tu disposes d'un accès complet à Internet et au web via 'web_search' (pour chercher sur le web) et 'web_fetch' (pour lire une page web). \
Utilise également les autres outils mis à disposition (terminal_exec, create_artifact, store_memory, recall_memory) quand nécessaire. \
Sous Windows, 'terminal_exec' s'exécute dans PowerShell : utilise ';' et jamais '&&'. \
N'hésite JAMAIS à utiliser 'web_search' quand l'utilisateur demande des informations récentes, des cotations, des recettes ou des données extérieures. \
En cours de mission, n'émets pas de phrase d'annonce sans appel d'outil : appelle directement l'outil suivant. \
Quand ta mission est entièrement accomplie et vérifiée, fournis directement ta réponse finale complète sans appel d'outil.";

        let mut messages = vec![
            ChatMessage { role: "system".to_string(), content: system_prompt.to_string(), images: None },
            ChatMessage { role: "user".to_string(), content: prompt.to_string(), images: None },
        ];

        let tools = get_primitive_tools_schemas();
        let mut step_order = 1;
        let mut total_tokens = 0;
        let mut final_answer = String::new();
        let mut active_backend_name = String::from("rust-native");
        static GLOBAL_TOOL_CACHE: OnceLock<Mutex<tool_output_cache::ToolCache>> = OnceLock::new();
        let cache_mutex = GLOBAL_TOOL_CACHE.get_or_init(|| Mutex::new(tool_output_cache::ToolCache::new(128)));
        let mut llm_budget = retry_budget::RetryBudget::new(10, 5);

        let mut step_budget = self.max_steps;
        let mut budget_recredited = false;
        let mut empty_tool_nudges: usize = 0;
        let mut step_idx = 0usize;
        while step_idx < step_budget {
            step_idx += 1;
            compact_messages(&mut messages, 60_000);
            if !llm_budget.is_retry_allowed() {
                return Err("Circuit breaker LLM ouvert : trop d'échecs consécutifs dans la fenêtre glissante.".to_string());
            }
            let (chat_res, active_backend) = match self.router.chat_completion_with_fallback(&messages, Some(&tools)) {
                Ok(res) => {
                    llm_budget.record_success();
                    res
                }
                Err(e) => {
                    llm_budget.record_failure();
                    return Err(e);
                }
            };
            active_backend_name = active_backend;
            total_tokens += chat_res.total_tokens;

            if !chat_res.content.is_empty() {
                self.record_trace(&run_id, step_order, "thought", None, &chat_res.content);
                on_trace(&run_id, step_order, "thought", None, &chat_res.content, &active_backend_name);
                step_order += 1;
                messages.push(ChatMessage { role: "assistant".to_string(), content: chat_res.content.clone(), images: None });
            }

            // S'il n'y a aucun appel d'outil, vérifier si le modèle s'est juste arrêté sur une annonce intermédiaire
            if chat_res.tool_calls.is_empty() {
                if empty_tool_nudges < 3 && looks_like_unfinished_thought(&chat_res.content) {
                    empty_tool_nudges += 1;
                    println!("  🔄 [ANTI-STALL] Annonce sans outil ({}/3), relance automatique...", empty_tool_nudges);
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: "Continue : n'annonce pas ton action en texte seul, appelle immédiatement l'outil nécessaire (ou fournis ta réponse finale complète si toute la mission est terminée et vérifiée).".to_string(),
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

            // Exécution des outils demandés
            for call in chat_res.tool_calls {
                let fn_name = &call.name;
                let fn_args = &call.arguments;

                self.record_trace(&run_id, step_order, "tool_call", Some(fn_name), &fn_args.to_string());
                on_trace(&run_id, step_order, "tool_call", Some(fn_name), &fn_args.to_string(), &active_backend_name);
                step_order += 1;

                println!("  ⚡ [OUTIL RUST] {} {:?}", fn_name, fn_args);
                let args_key = fn_args.to_string();
                let is_cacheable_read = matches!(fn_name.as_str(), "recall_memory" | "read_file" | "list_files" | "web_fetch");
                let raw_output = if is_cacheable_read {
                    let hit = cache_mutex.lock().ok().and_then(|mut c| c.get(fn_name, &args_key).map(|s| s.to_string()));
                    if let Some(cached) = hit {
                        println!("  ♻️ [TOOL CACHE] HIT {} {}", fn_name, args_key);
                        cached
                    } else {
                        self.dispatch_tool(fn_name, fn_args, &run_id)
                    }
                } else {
                    self.dispatch_tool(fn_name, fn_args, &run_id)
                };
                let tool_output = context_trimmer::truncate_tool_output(&raw_output, 16_000);
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

            if step_idx == step_budget && !budget_recredited {
                budget_recredited = true;
                step_budget += 8;
                println!("  🔋 [REACT] Budget initial atteint en cours d'exécution, recrédit automatique (+8 tours -> {})", step_budget);
                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: "Budget prolongé (+8 tours) : finalise ta tâche, vérifie tes livrables et donne ta réponse finale.".to_string(),
                    images: None,
                });
            }
        }

        if final_answer.is_empty() {
            final_answer = format!("⚠️ Limite de {} tours atteinte sans réponse finale.", step_budget);
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
