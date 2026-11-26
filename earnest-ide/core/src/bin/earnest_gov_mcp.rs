//! `earnest_gov_mcp` — Serveur MCP stdio exposant la gouvernance par invariants
//! et `shadow_worktree` d'Earnest IDE aux agents externes (Claude Code, Cursor, Codex, etc.).

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use earnest_ide_core::governance::{
    seal_invariants, verify_invariants_lock, ContractSpec, GovernanceMode, InvariantsFile,
    PolicyManager, ProbeSpec, ShadowRunner,
};
use serde_json::{json, Value};

struct GovMcpServer {
    workspace_root: PathBuf,
}

impl GovMcpServer {
    fn new(workspace_root: impl AsRef<Path>) -> Self {
        Self {
            workspace_root: workspace_root.as_ref().to_path_buf(),
        }
    }

    fn tool_definitions() -> Vec<Value> {
        vec![
            json!({
                "name": "gov_status",
                "description": "Retourne le mode de gouvernance actuel (normal/constrained/architecture), l'intégrité SHA-256 du scellement, les contrats et les fichiers stagés dans shadow_worktree.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
            json!({
                "name": "gov_add_invariant",
                "description": "Ajoute ou met à jour un contrat d'invariant dans .ide/invariants.json.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Identifiant court (ex: C1, C2)" },
                        "target_file": { "type": "string", "description": "Fichier cible (ex: src/lib.rs, Cargo.toml)" },
                        "test_query": { "type": "string", "description": "Nom du test (cargo_test) ou marqueur/symbole (static)" },
                        "runner": { "type": "string", "description": "'cargo_test' (défaut) ou 'static'" },
                        "criticality": { "type": "string", "description": "'high' (défaut), 'medium' ou 'low'" }
                    },
                    "required": ["id", "target_file", "test_query"]
                }
            }),
            json!({
                "name": "gov_seal_architecture",
                "description": "Scelle .ide/invariants.json avec SHA-256 (.ide/invariants.lock en lecture seule) et bascule le workspace en Mode 2 Contraint.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
            json!({
                "name": "gov_write_code",
                "description": "Écrit un fichier dans .ide/shadow_worktree (stage_only=true par défaut) ou valide immédiatement contre le contrat scellé et fusionne.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "target_contract_id": { "type": "string", "description": "Contrat satisfait (ex: C1)" },
                        "target_file": { "type": "string", "description": "Chemin relatif du fichier cible" },
                        "content": { "type": "string", "description": "Contenu complet à écrire" },
                        "stage_only": { "type": "boolean", "description": "Si true (défaut), stage sans lancer les sondes" }
                    },
                    "required": ["target_contract_id", "target_file", "content"]
                }
            }),
            json!({
                "name": "gov_shadow_check",
                "description": "Exécute cargo test / cargo clippy / pytest / node --test dans .ide/shadow_worktree sans fusionner.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "filter": { "type": "string", "description": "Filtre optionnel de test" },
                        "check_only": { "type": "boolean" },
                        "clippy": { "type": "boolean" }
                    }
                }
            }),
            json!({
                "name": "gov_shadow_validate_and_merge",
                "description": "Exécute les sondes du contrat scellé + project-integrity-gate dans shadow_worktree et fusionne vers le workspace réel si tout passe.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "target_contract_id": { "type": "string", "description": "Identifiant du contrat (ex: C1)" }
                    },
                    "required": ["target_contract_id"]
                }
            }),
        ]
    }

    fn load_invariants(&self, inv_rel: &str) -> InvariantsFile {
        let p = self.workspace_root.join(inv_rel);
        std::fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn call_tool(&self, name: &str, args: &Value) -> Result<Value, String> {
        let pm = PolicyManager::new(&self.workspace_root);
        let policy = pm.load_or_init(None)?;

        match name {
            "gov_status" => {
                let lock_status = verify_invariants_lock(
                    &self.workspace_root,
                    &policy.invariants_file,
                    &policy.lock_file,
                );
                let inv = self.load_invariants(&policy.invariants_file);
                let runner = ShadowRunner::new(
                    &self.workspace_root,
                    &policy.shadow_env.path,
                    policy.shadow_env.timeout_ms,
                );
                let staged = runner.list_shadow_staged_files();
                Ok(json!({
                    "mode": format!("{:?}", policy.mode).to_lowercase(),
                    "contracts_count": inv.contracts.len(),
                    "lock_valid": lock_status.is_ok(),
                    "lock_hash": lock_status.ok(),
                    "staged_files": staged,
                }))
            }
            "gov_add_invariant" => {
                let id = args["id"].as_str().unwrap_or("").trim();
                let target_file = args["target_file"].as_str().unwrap_or("").trim();
                let test_query = args["test_query"].as_str().unwrap_or("").trim();
                if id.is_empty() || target_file.is_empty() || test_query.is_empty() {
                    return Err("id, target_file et test_query sont requis".to_string());
                }
                let runner_name = args["runner"].as_str().unwrap_or("cargo_test").to_string();
                let criticality = args["criticality"].as_str().unwrap_or("high").to_string();

                let mut inv = self.load_invariants(&policy.invariants_file);
                let contract = ContractSpec {
                    id: id.to_string(),
                    criticality,
                    target_file: target_file.to_string(),
                    dependencies: Vec::new(),
                    symbols: Vec::new(),
                    probes: vec![ProbeSpec {
                        probe_id: format!("{}-P1", id),
                        runner: runner_name,
                        query: Some(test_query.to_string()),
                        command: None,
                        expect: json!("pass"),
                    }],
                };
                if let Some(existing) = inv.contracts.iter_mut().find(|c| c.id == id) {
                    *existing = contract;
                } else {
                    inv.contracts.push(contract);
                }

                let inv_path = self.workspace_root.join(&policy.invariants_file);
                if let Some(p) = inv_path.parent() {
                    std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
                }
                let raw = serde_json::to_string_pretty(&inv).map_err(|e| e.to_string())?;
                std::fs::write(&inv_path, raw).map_err(|e| e.to_string())?;
                Ok(json!({ "status": "ok", "id": id, "contracts_count": inv.contracts.len() }))
            }
            "gov_seal_architecture" => {
                let inv = self.load_invariants(&policy.invariants_file);
                if inv.contracts.is_empty() {
                    return Err("Aucun contrat dans .ide/invariants.json : ajoutez au moins un invariant avant de sceller.".to_string());
                }
                let hash = seal_invariants(
                    &self.workspace_root,
                    &policy.invariants_file,
                    &policy.lock_file,
                    &inv,
                )?;
                pm.load_or_init(Some(GovernanceMode::Constrained))?;
                Ok(json!({ "status": "sealed", "mode": "constrained", "sha256": hash }))
            }
            "gov_write_code" => {
                let cid = args["target_contract_id"].as_str().unwrap_or("");
                let target_file = args["target_file"].as_str().unwrap_or("");
                let content = args["content"].as_str().unwrap_or("");
                let stage_only = args["stage_only"].as_bool().unwrap_or(true);

                verify_invariants_lock(
                    &self.workspace_root,
                    &policy.invariants_file,
                    &policy.lock_file,
                )?;
                let rel = ShadowRunner::normalize_rel_path(&self.workspace_root, target_file)?;
                let runner = ShadowRunner::new(
                    &self.workspace_root,
                    &policy.shadow_env.path,
                    policy.shadow_env.timeout_ms,
                );
                if stage_only {
                    runner.prepare_shadow_worktree()?;
                    let dest = self
                        .workspace_root
                        .join(&policy.shadow_env.path)
                        .join(&rel);
                    if let Some(p) = dest.parent() {
                        std::fs::create_dir_all(p).ok();
                    }
                    std::fs::write(&dest, content).map_err(|e| e.to_string())?;
                    return Ok(json!({ "status": "STAGED", "file": rel }));
                }
                let inv = self.load_invariants(&policy.invariants_file);
                let contract = inv
                    .contracts
                    .iter()
                    .find(|c| c.id == cid)
                    .ok_or_else(|| format!("Contrat '{}' introuvable", cid))?;
                let res = runner.execute_contract_patch(contract, &rel, content);
                Ok(serde_json::to_value(res).unwrap_or(json!({})))
            }
            "gov_shadow_check" => {
                let runner = ShadowRunner::new(
                    &self.workspace_root,
                    &policy.shadow_env.path,
                    policy.shadow_env.timeout_ms,
                );
                let filter = args["filter"].as_str();
                let check_only = args["check_only"].as_bool().unwrap_or(false);
                let clippy = args["clippy"].as_bool().unwrap_or(false);
                runner.run_shadow_check(filter, check_only, clippy)
            }
            "gov_shadow_validate_and_merge" => {
                let cid = args["target_contract_id"].as_str().unwrap_or("");
                verify_invariants_lock(
                    &self.workspace_root,
                    &policy.invariants_file,
                    &policy.lock_file,
                )?;
                let inv = self.load_invariants(&policy.invariants_file);
                let contract = inv
                    .contracts
                    .iter()
                    .find(|c| c.id == cid)
                    .ok_or_else(|| format!("Contrat '{}' introuvable", cid))?;
                let runner = ShadowRunner::new(
                    &self.workspace_root,
                    &policy.shadow_env.path,
                    policy.shadow_env.timeout_ms,
                );
                let res = runner.execute_contract_patches(contract, &[], &contract.target_file);
                Ok(serde_json::to_value(res).unwrap_or(json!({})))
            }
            other => Err(format!("Outil gov MCP inconnu: '{}'", other)),
        }
    }

    fn handle(&self, request: &Value) -> Option<Value> {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str)?;
        match method {
            "initialize" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "earnest-gov-mcp", "version": env!("CARGO_PKG_VERSION") }
                }
            })),
            "notifications/initialized" => None,
            "tools/list" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": Self::tool_definitions() }
            })),
            "tools/call" => {
                let name = request
                    .pointer("/params/name")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let args = request
                    .pointer("/params/arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
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

fn main() {
    let mut workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--workspace" {
            if let Some(v) = args.next() {
                workspace = PathBuf::from(v);
            }
        }
    }

    let server = GovMcpServer::new(&workspace);
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(req) = serde_json::from_str::<Value>(trimmed) {
            if let Some(resp) = server.handle(&req) {
                let _ = writeln!(stdout, "{}", resp);
                let _ = stdout.flush();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gov_mcp_initialize_add_seal_and_status_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("earnest_gov_mcp_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let srv = GovMcpServer::new(&tmp);

        let init = srv
            .handle(&json!({"jsonrpc":"2.0","id":1,"method":"initialize"}))
            .unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "earnest-gov-mcp");

        let add = srv
            .call_tool(
                "gov_add_invariant",
                &json!({
                    "id": "C1",
                    "target_file": "Cargo.toml",
                    "test_query": "zero_dep",
                    "runner": "static"
                }),
            )
            .unwrap();
        assert_eq!(add["contracts_count"], 1);

        let seal = srv.call_tool("gov_seal_architecture", &json!({})).unwrap();
        assert_eq!(seal["status"], "sealed");
        assert_eq!(seal["mode"], "constrained");

        let status = srv.call_tool("gov_status", &json!({})).unwrap();
        assert_eq!(status["lock_valid"], true);
        assert_eq!(status["mode"], "constrained");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
