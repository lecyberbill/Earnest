//! Client MCP (Model Context Protocol) JSON-RPC 2.0 via stdio pour `earnest-core`.
//!
//! Permet à `ReActEngine` et aux endpoints `/api/tools`, `/api/tools/test`, `/api/mcp/test`
//! de découvrir et d'exécuter les outils exposés par les serveurs configurés dans
//! `app_settings.mcp_servers_config`.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub server_name: String,
    pub raw_name: String,
    pub full_name: String,
    pub description: String,
    pub input_schema: Value,
}

pub struct McpStdioClient {
    server_name: String,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl McpStdioClient {
    /// Démarre le serveur MCP en sous-processus stdio et effectue le handshake `initialize`.
    pub fn spawn(config: &McpServerConfig) -> Result<Self, String> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd.spawn().map_err(|e| {
            format!(
                "Impossible de démarrer le serveur MCP '{}' ({}): {}",
                config.name, config.command, e
            )
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("stdin indisponible pour '{}'", config.name))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| format!("stdout indisponible pour '{}'", config.name))?;

        let mut client = Self {
            server_name: config.name.clone(),
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
        };

        let _ = client.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "earnest-core", "version": env!("CARGO_PKG_VERSION") }
            }),
        )?;
        client.notify("notifications/initialized", json!({}))?;
        Ok(client)
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        writeln!(self.stdin, "{}", msg).map_err(|e| format!("MCP write error: {}", e))?;
        self.stdin
            .flush()
            .map_err(|e| format!("MCP flush error: {}", e))?;
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        writeln!(self.stdin, "{}", req).map_err(|e| format!("MCP write error: {}", e))?;
        self.stdin
            .flush()
            .map_err(|e| format!("MCP flush error: {}", e))?;

        let mut line = String::new();
        loop {
            line.clear();
            let n = self
                .stdout
                .read_line(&mut line)
                .map_err(|e| format!("MCP read error ({}): {}", self.server_name, e))?;
            if n == 0 {
                return Err(format!(
                    "Le serveur MCP '{}' a fermé stdout de manière inattendue.",
                    self.server_name
                ));
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let val: Value = serde_json::from_str(trimmed).map_err(|e| {
                format!("Réponse JSON-RPC invalide de '{}': {}", self.server_name, e)
            })?;
            if val.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(err) = val.get("error") {
                    return Err(format!(
                        "Erreur MCP '{}' ({}): {}",
                        self.server_name,
                        err["code"],
                        err["message"].as_str().unwrap_or("")
                    ));
                }
                return Ok(val.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }

    /// Liste les outils exposés via `tools/list` et préfixe avec `mcp__<server>__`.
    pub fn list_tools(&mut self) -> Result<Vec<McpToolInfo>, String> {
        let res = self.request("tools/list", json!({}))?;
        let arr = res["tools"].as_array().cloned().unwrap_or_default();
        let mut out = Vec::with_capacity(arr.len());
        for t in arr {
            let raw_name = t["name"].as_str().unwrap_or("").to_string();
            if raw_name.is_empty() {
                continue;
            }
            let full_name = format!("mcp__{}__{}", self.server_name, raw_name);
            let description = t["description"].as_str().unwrap_or("").to_string();
            let input_schema = t
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
            out.push(McpToolInfo {
                server_name: self.server_name.clone(),
                raw_name,
                full_name,
                description,
                input_schema,
            });
        }
        Ok(out)
    }

    /// Exécute un outil via `tools/call`.
    pub fn call_tool(&mut self, raw_name: &str, arguments: &Value) -> Result<Value, String> {
        self.request(
            "tools/call",
            json!({
                "name": raw_name,
                "arguments": arguments
            }),
        )
    }
}

impl Drop for McpStdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Charge la liste des serveurs MCP depuis `app_settings.mcp_servers_config`.
pub fn load_mcp_servers(conn: &Connection) -> Vec<McpServerConfig> {
    let Ok(mut stmt) =
        conn.prepare("SELECT value FROM app_settings WHERE key = 'mcp_servers_config'")
    else {
        return Vec::new();
    };
    let Ok(raw) = stmt.query_row([], |r| r.get::<_, String>(0)) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Découvre les outils de tous les serveurs MCP configurés (ignore les serveurs en échec).
pub fn discover_all_mcp_tools(conn: &Connection) -> Vec<McpToolInfo> {
    let mut all = Vec::new();
    for cfg in load_mcp_servers(conn) {
        if cfg.name.trim().is_empty() || cfg.command.trim().is_empty() {
            continue;
        }
        if let Ok(mut client) = McpStdioClient::spawn(&cfg) {
            if let Ok(tools) = client.list_tools() {
                all.extend(tools);
            }
        }
    }
    all
}

/// Convertit un outil MCP au format OpenAI Function Tool pour `ReActEngine`.
pub fn to_openai_tool_schema(tool: &McpToolInfo) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.full_name,
            "description": format!("[{} MCP] {}", tool.server_name, tool.description),
            "parameters": tool.input_schema
        }
    })
}

/// Découpe un nom préfixé `mcp__<server>__<tool>`.
pub fn parse_prefixed_name(full_name: &str) -> Option<(&str, &str)> {
    let rest = full_name.strip_prefix("mcp__")?;
    let (server, tool) = rest.split_once("__")?;
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    Some((server, tool))
}

/// Exécute un outil préfixé `mcp__<server>__<tool>` en résolvant le serveur dans SQLite.
pub fn call_prefixed_mcp_tool(
    conn: &Connection,
    full_name: &str,
    arguments: &Value,
) -> Result<Value, String> {
    let (server_name, raw_name) = parse_prefixed_name(full_name)
        .ok_or_else(|| format!("Nom d'outil MCP invalide: '{}'", full_name))?;
    let servers = load_mcp_servers(conn);
    let cfg = servers
        .into_iter()
        .find(|s| s.name == server_name)
        .ok_or_else(|| format!("Serveur MCP '{}' non configuré", server_name))?;
    let mut client = McpStdioClient::spawn(&cfg)?;
    client.call_tool(raw_name, arguments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prefixed_mcp_tool_names() {
        assert_eq!(
            parse_prefixed_name("mcp__wfl__wfl_validate"),
            Some(("wfl", "wfl_validate"))
        );
        assert_eq!(parse_prefixed_name("mcp__bad"), None);
        assert_eq!(parse_prefixed_name("web_search"), None);
    }

    #[test]
    fn converts_mcp_tool_to_openai_schema() {
        let info = McpToolInfo {
            server_name: "wfl".into(),
            raw_name: "wfl_validate".into(),
            full_name: "mcp__wfl__wfl_validate".into(),
            description: "Valide un YAML".into(),
            input_schema: json!({"type": "object", "properties": {"yaml": {"type": "string"}}}),
        };
        let schema = to_openai_tool_schema(&info);
        assert_eq!(schema["function"]["name"], "mcp__wfl__wfl_validate");
        assert_eq!(schema["function"]["description"], "[wfl MCP] Valide un YAML");
    }
}
