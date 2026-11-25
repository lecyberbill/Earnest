use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use wf_core::{
    generate_schema_json, parse_workflow_yaml, validate_workflow, ExecutionOptions, WfStore,
};

pub struct WfMcpServer {
    workspace_root: PathBuf,
    store: WfStore,
}

impl WfMcpServer {
    pub fn open(workspace_root: impl AsRef<Path>, db_path: impl AsRef<Path>) -> Result<Self, String> {
        let store = WfStore::open(db_path.as_ref())?;
        Ok(Self {
            workspace_root: workspace_root.as_ref().to_path_buf(),
            store,
        })
    }

    pub fn tool_definitions() -> Vec<Value> {
        vec![
            json!({
                "name": "wfl_get_grammar",
                "description": "Retourne le schéma JSON WFL v0.1 et la liste des workflows enregistrés.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
            json!({
                "name": "wfl_validate",
                "description": "Parse un YAML WFL brut et exécute la validation sémantique (retourne VALID + sha256 ou la liste d'erreurs).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "yaml": { "type": "string", "description": "Contenu YAML du workflow" }
                    },
                    "required": ["yaml"]
                }
            }),
            json!({
                "name": "wfl_save_draft",
                "description": "Valide et enregistre un workflow en statut 'draft' dans le workspace et SQLite WAL.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "yaml": { "type": "string", "description": "Contenu YAML du workflow" },
                        "file_name": { "type": "string", "description": "Nom de fichier optionnel (défaut: <name>.wf.yaml)" }
                    },
                    "required": ["yaml"]
                }
            }),
            json!({
                "name": "wfl_seal",
                "description": "Scelle un workflow validé (SHA-256) pour autoriser son exécution réelle (dry_run=false).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Nom du workflow à sceller" }
                    },
                    "required": ["name"]
                }
            }),
            json!({
                "name": "wfl_run",
                "description": "Exécute un workflow enregistré (dry_run=true requis tant que draft) et journalise dans SQLite WAL.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Nom du workflow" },
                        "dry_run": { "type": "boolean", "description": "Simulation sans écriture disque (défaut: true)" },
                        "inputs": { "type": "object", "description": "Paramètres d'entrée optionnels" }
                    },
                    "required": ["name"]
                }
            }),
            json!({
                "name": "wfl_inspect_run",
                "description": "Inspecte un run_id dans SQLite WAL (statut et step_logs détaillés).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "run_id": { "type": "string", "description": "Identifiant du run" }
                    },
                    "required": ["run_id"]
                }
            }),
            json!({
                "name": "wfl_list_workflows",
                "description": "Liste les workflows enregistrés et leur statut (draft / sealed), dossier, activation et cron.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
            json!({
                "name": "wfl_set_enabled",
                "description": "Active ou désactive un workflow enregistré (bloque l'exécution réelle si désactivé).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Nom du workflow" },
                        "enabled": { "type": "boolean", "description": "true pour activer, false pour désactiver" }
                    },
                    "required": ["name", "enabled"]
                }
            }),
            json!({
                "name": "wfl_set_folder",
                "description": "Classe un workflow dans un dossier ou groupe (ex: 'General', 'Prod/Finance').",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Nom du workflow" },
                        "folder": { "type": "string", "description": "Nom du dossier/groupe" }
                    },
                    "required": ["name", "folder"]
                }
            }),
            json!({
                "name": "wfl_set_cron",
                "description": "Associe ou supprime une planification cron sur un workflow.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Nom du workflow" },
                        "cron_expr": { "type": ["string", "null"], "description": "Expression cron 5 champs (ex: '0 2 * * *') ou null pour désactiver" }
                    },
                    "required": ["name"]
                }
            }),
            json!({
                "name": "wfl_list_runs",
                "description": "Liste l'historique des runs WFL enregistrés dans SQLite WAL.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workflow_name": { "type": "string", "description": "Filtrer par nom de workflow (optionnel)" },
                        "limit": { "type": "integer", "description": "Nombre max de runs (défaut: 20)" }
                    }
                }
            }),
        ]
    }

    pub fn call_tool(&self, name: &str, args: &Value) -> Result<Value, String> {
        match name {
            "wfl_get_grammar" => {
                let schema_str = generate_schema_json();
                let schema: Value = serde_json::from_str(&schema_str).unwrap_or(json!({}));
                let workflows = self.store.list_workflows()?;
                Ok(json!({
                    "version": "0.1",
                    "schema": schema,
                    "workflows": workflows
                }))
            }
            "wfl_validate" => {
                let yaml = args["yaml"].as_str().unwrap_or("");
                let spec = parse_workflow_yaml(yaml)?;
                let report = validate_workflow(&spec);
                let errors: Vec<Value> = report
                    .errors
                    .iter()
                    .map(|e| json!({"code": e.code, "path": e.path, "message": e.message}))
                    .collect();
                Ok(json!({
                    "valid": report.valid,
                    "sha256": report.sha256,
                    "step_count": report.step_count,
                    "errors": errors
                }))
            }
            "wfl_save_draft" => {
                let yaml = args["yaml"].as_str().unwrap_or("");
                let spec = parse_workflow_yaml(yaml)?;
                let file_name = args["file_name"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("{}.wf.yaml", spec.name));
                let dest = self.workspace_root.join("workflows").join(file_name);
                let rec = self.store.save_draft(&spec, &dest)?;
                Ok(serde_json::to_value(rec).unwrap_or(json!({})))
            }
            "wfl_seal" => {
                let wf_name = args["name"].as_str().unwrap_or("");
                let rec = self.store.seal_workflow(wf_name)?;
                Ok(serde_json::to_value(rec).unwrap_or(json!({})))
            }
            "wfl_run" => {
                let wf_name = args["name"].as_str().unwrap_or("");
                let dry_run = args["dry_run"].as_bool().unwrap_or(true);
                let inputs: HashMap<String, Value> = args["inputs"]
                    .as_object()
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default();
                let (run_id, report) = self.store.execute_and_record(
                    wf_name,
                    ExecutionOptions {
                        inputs,
                        dry_run,
                        ..Default::default()
                    },
                )?;
                Ok(json!({
                    "run_id": run_id,
                    "report": report
                }))
            }
            "wfl_inspect_run" => {
                let run_id = args["run_id"].as_str().unwrap_or("");
                let details = self.store.inspect_run(run_id)?;
                Ok(json!({ "found": details.is_some(), "run": details }))
            }
            "wfl_list_workflows" => {
                let list = self.store.list_workflows()?;
                Ok(json!({ "workflows": list }))
            }
            "wfl_set_enabled" => {
                let wf_name = args["name"].as_str().unwrap_or("");
                let enabled = args["enabled"].as_bool().unwrap_or(true);
                self.store.set_workflow_enabled(wf_name, enabled)?;
                Ok(json!({ "name": wf_name, "enabled": enabled }))
            }
            "wfl_set_folder" => {
                let wf_name = args["name"].as_str().unwrap_or("");
                let folder = args["folder"].as_str().unwrap_or("General");
                self.store.set_workflow_folder(wf_name, folder)?;
                Ok(json!({ "name": wf_name, "folder": folder }))
            }
            "wfl_set_cron" => {
                let wf_name = args["name"].as_str().unwrap_or("");
                let cron = args["cron_expr"].as_str();
                self.store.set_workflow_cron(wf_name, cron)?;
                let (desc_fr, next_unix) = cron
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .and_then(|c| wf_core::parse_wfl_cron(c).ok())
                    .map(|s| {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        (Some(s.describe_fr()), s.next_after(now))
                    })
                    .unwrap_or((None, None));
                Ok(json!({
                    "name": wf_name,
                    "cron_expr": cron,
                    "description_fr": desc_fr,
                    "next_run_unix": next_unix
                }))
            }
            "wfl_list_runs" => {
                let wf_name = args["workflow_name"].as_str();
                let limit = args["limit"].as_u64().unwrap_or(20) as usize;
                let runs = self.store.list_runs(wf_name, limit)?;
                Ok(json!({ "runs": runs }))
            }
            other => Err(format!("Outil WFL MCP inconnu: '{}'", other)),
        }
    }

    pub fn handle(&self, request: &Value) -> Option<Value> {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str)?;
        match method {
            "initialize" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "wf-mcp", "version": env!("CARGO_PKG_VERSION") }
                }
            })),
            "notifications/initialized" => None,
            "tools/list" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": Self::tool_definitions() }
            })),
            "tools/call" => {
                let name = request.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
                let args = request.pointer("/params/arguments").cloned().unwrap_or_else(|| json!({}));
                let res = match self.call_tool(name, &args) {
                    Ok(v) => json!({
                        "content": [{"type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default()}],
                        "structuredContent": v,
                        "isError": false
                    }),
                    Err(e) => json!({
                        "content": [{"type": "text", "text": e}],
                        "isError": true
                    }),
                };
                Some(json!({"jsonrpc": "2.0", "id": id, "result": res}))
            }
            _ => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("Méthode inconnue: {}", method)}
            })),
        }
    }
}
