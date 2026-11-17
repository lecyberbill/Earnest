//! Module MCP (Model Context Protocol) JSON-RPC 2.0 pour `earnest-core`.
//!
//! Expose `earnest-core` comme serveur MCP stdio (`earnest-core --mcp`)
//! permettant à tout client MCP (Antigravity, Claude, Cursor, etc.) de piloter
//! les agents autonomes `ReActEngine`, la mémoire SQLite et le sondage LLM.

use std::io::{self, BufRead, Write};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

use crate::db::memories::MemoryService;
use crate::engine::react::ReActEngine;
use crate::llm::router::LLMRouter;

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Retourne le catalogue des outils MCP exposés par `earnest-core`.
pub fn list_mcp_tools() -> Value {
    json!({
        "tools": [
            {
                "name": "earnest_run",
                "description": "Exécute une mission autonome complète via le moteur ReAct d'earnest-core et retourne la réponse finale, les tokens et la durée.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string", "description": "Consigne ou tâche à exécuter par l'agent autonome" },
                        "project": { "type": "string", "description": "Identifiant du projet (défaut: default_workspace)" },
                        "max_steps": { "type": "integer", "description": "Nombre maximal d'itérations ReAct (défaut: 8)" }
                    },
                    "required": ["prompt"]
                }
            },
            {
                "name": "earnest_probe_models",
                "description": "Sonde tous les backends LLM déclarés (DeepSeek, Ollama, LM Studio, OpenAI) et retourne leur statut et latence.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "earnest_store_memory",
                "description": "Persiste une information clé dans la mémoire SQLite d'earnest-core.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "description": "Clé d'identification" },
                        "content": { "type": "string", "description": "Contenu textuel ou Markdown" },
                        "project": { "type": "string", "description": "Projet cible (défaut: default_workspace)" },
                        "scope": { "type": "string", "enum": ["project", "session"], "description": "Portée (défaut: project)" }
                    },
                    "required": ["key", "content"]
                }
            },
            {
                "name": "earnest_recall_memory",
                "description": "Lit une mémoire précédemment enregistrée dans SQLite par sa clé.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "description": "Clé à rechercher" },
                        "project": { "type": "string", "description": "Projet cible (défaut: default_workspace)" }
                    },
                    "required": ["key"]
                }
            },
            {
                "name": "earnest_list_runs",
                "description": "Liste les derniers runs exécutés par les agents autonomes dans SQLite.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": { "type": "integer", "description": "Nombre max de runs à retourner (défaut: 10)" }
                    }
                }
            }
        ]
    })
}

/// Traite une requête JSON-RPC 2.0 MCP unique.
/// Retourne `None` pour les notifications (pas d'`id`).
pub fn handle_mcp_request(conn: &Connection, req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");

    // Notifications (ex: notifications/initialized) : aucune réponse attendue.
    if id.is_none() || method.starts_with("notifications/") {
        return None;
    }
    let id = id.unwrap();

    let result: Result<Value, (i64, String)> = match method {
        "initialize" => Ok(json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "serverInfo": {
                "name": "earnest-core",
                "version": env!("CARGO_PKG_VERSION")
            }
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(list_mcp_tools()),
        "tools/call" => {
            let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let raw_args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let args = match raw_args {
                Value::String(s) => {
                    let repaired = json_repair::repair(&s);
                    serde_json::from_str(&repaired).unwrap_or_else(|_| json!({}))
                }
                other => other,
            };
            Ok(execute_mcp_tool(conn, tool_name, &args))
        }
        other => Err((-32601, format!("Méthode MCP inconnue: {}", other))),
    };

    Some(match result {
        Ok(res) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": res
        }),
        Err((code, msg)) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": msg }
        }),
    })
}

fn execute_mcp_tool(conn: &Connection, tool_name: &str, args: &Value) -> Value {
    let text_result = |txt: String, is_error: bool| -> Value {
        json!({
            "content": [{ "type": "text", "text": txt }],
            "isError": is_error
        })
    };

    match tool_name {
        "earnest_run" => {
            let prompt = args["prompt"].as_str().unwrap_or("");
            if prompt.trim().is_empty() {
                return text_result("Paramètre 'prompt' requis.".to_string(), true);
            }
            let project = args["project"].as_str().unwrap_or("default_workspace");
            let router = LLMRouter::from_db(conn);
            let mut engine = ReActEngine::new(conn, &router, project);
            if let Some(ms) = args["max_steps"].as_u64() {
                engine.max_steps = (ms as usize).clamp(1, 32);
            }
            match engine.run(prompt) {
                Ok(res) => {
                    let payload = json!({
                        "run_id": res.run_id,
                        "status": res.status,
                        "answer": res.answer,
                        "steps_count": res.steps_count,
                        "total_tokens": res.total_tokens,
                        "duration_ms": res.duration_ms
                    });
                    text_result(serde_json::to_string_pretty(&payload).unwrap_or_default(), false)
                }
                Err(e) => text_result(format!("Erreur ReAct: {}", e), true),
            }
        }
        "earnest_probe_models" => {
            let router = LLMRouter::from_db(conn);
            let probes: Vec<Value> = router
                .probe_all()
                .into_iter()
                .map(|p| {
                    json!({
                        "name": p.name,
                        "provider_type": p.provider_type,
                        "model": p.model,
                        "is_online": p.is_online,
                        "latency_ms": p.latency_ms,
                        "error": p.error
                    })
                })
                .collect();
            text_result(
                serde_json::to_string_pretty(&json!({ "models": probes })).unwrap_or_default(),
                false,
            )
        }
        "earnest_store_memory" => {
            let key = args["key"].as_str().unwrap_or("");
            let content = args["content"].as_str().unwrap_or("");
            if key.is_empty() || content.is_empty() {
                return text_result("Paramètres 'key' et 'content' requis.".to_string(), true);
            }
            let project = args["project"].as_str().unwrap_or("default_workspace");
            let scope = args["scope"].as_str().unwrap_or("project");
            let mem = MemoryService::new(conn);
            match mem.store_memory(project, key, content, scope, None) {
                Ok(_) => text_result(format!("Mémoire '{}' enregistrée (projet '{}').", key, project), false),
                Err(e) => text_result(format!("Erreur SQLite store_memory: {}", e), true),
            }
        }
        "earnest_recall_memory" => {
            let key = args["key"].as_str().unwrap_or("");
            if key.is_empty() {
                return text_result("Paramètre 'key' requis.".to_string(), true);
            }
            let project = args["project"].as_str().unwrap_or("default_workspace");
            let mem = MemoryService::new(conn);
            match mem.recall_memory(project, key, None) {
                Ok(Some(val)) => text_result(val, false),
                Ok(None) => text_result(format!("Aucune mémoire trouvée pour '{}'.", key), false),
                Err(e) => text_result(format!("Erreur SQLite recall_memory: {}", e), true),
            }
        }
        "earnest_list_runs" => {
            let limit = args["limit"].as_i64().unwrap_or(10).clamp(1, 50);
            let mut stmt = match conn.prepare(
                "SELECT id, project_id, status, duration_ms, total_tokens, created_at FROM runs ORDER BY created_at DESC LIMIT ?1",
            ) {
                Ok(s) => s,
                Err(e) => return text_result(format!("Erreur SQL: {}", e), true),
            };
            let rows = stmt
                .query_map(params![limit], |row| {
                    Ok(json!({
                        "id": row.get::<_, String>(0)?,
                        "project_id": row.get::<_, String>(1)?,
                        "status": row.get::<_, String>(2)?,
                        "duration_ms": row.get::<_, Option<i64>>(3)?,
                        "total_tokens": row.get::<_, Option<i64>>(4)?,
                        "created_at": row.get::<_, Option<String>>(5)?
                    }))
                })
                .ok();
            let list: Vec<Value> = rows
                .map(|r| r.filter_map(|item| item.ok()).collect())
                .unwrap_or_default();
            text_result(
                serde_json::to_string_pretty(&json!({ "runs": list })).unwrap_or_default(),
                false,
            )
        }
        other => text_result(format!("Outil MCP inconnu: {}", other), true),
    }
}

/// Boucle serveur MCP stdio (1 message JSON-RPC par ligne sur stdin/stdout).
pub fn serve_stdio(conn: &Connection) -> Result<(), String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line_res in stdin.lock().lines() {
        let line = line_res.map_err(|e| format!("Erreur lecture stdin MCP: {}", e))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => {
                let repaired = json_repair::repair(trimmed);
                match serde_json::from_str(&repaired) {
                    Ok(v) => v,
                    Err(e) => {
                        let err_resp = json!({
                            "jsonrpc": "2.0",
                            "id": Value::Null,
                            "error": { "code": -32700, "message": format!("Parse error: {}", e) }
                        });
                        writeln!(out, "{}", err_resp).ok();
                        out.flush().ok();
                        continue;
                    }
                }
            }
        };

        if let Some(resp) = handle_mcp_request(conn, &parsed) {
            writeln!(out, "{}", resp).map_err(|e| format!("Erreur écriture stdout MCP: {}", e))?;
            out.flush().map_err(|e| format!("Erreur flush stdout MCP: {}", e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connection::init_db;

    #[test]
    fn mcp_initialize_and_tools_list() {
        let conn = init_db(Some(std::path::Path::new(":memory:"))).unwrap();
        let init_req = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}});
        let init_resp = handle_mcp_request(&conn, &init_req).unwrap();
        assert_eq!(init_resp["result"]["serverInfo"]["name"], "earnest-core");

        let notif = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert!(handle_mcp_request(&conn, &notif).is_none());

        let list_req = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
        let list_resp = handle_mcp_request(&conn, &list_req).unwrap();
        let tools = list_resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 5);
    }

    #[test]
    fn mcp_store_and_recall_memory_with_repaired_args() {
        let conn = init_db(Some(std::path::Path::new(":memory:"))).unwrap();
        let store_req = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "earnest_store_memory",
                "arguments": "{\"key\": \"mcp_k\", \"content\": \"hello mcp\","
            }
        });
        let store_resp = handle_mcp_request(&conn, &store_req).unwrap();
        assert_eq!(store_resp["result"]["isError"], false);

        let recall_req = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "earnest_recall_memory",
                "arguments": { "key": "mcp_k" }
            }
        });
        let recall_resp = handle_mcp_request(&conn, &recall_req).unwrap();
        assert_eq!(recall_resp["result"]["content"][0]["text"], "hello mcp");
    }
}
