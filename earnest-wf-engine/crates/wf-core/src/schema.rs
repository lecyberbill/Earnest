use crate::ast::WorkflowSpec;
use schemars::schema_for;

/// Génère le JSON Schema officiel de `WorkflowSpec` (`wfl-v0.1.schema.json`).
pub fn generate_schema_json() -> String {
    let schema = schema_for!(WorkflowSpec);
    serde_json::to_string_pretty(&schema).unwrap_or_else(|_| "{}".to_string())
}
