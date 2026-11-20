//! `wf-core` — WFL (Workflow Language) AST, YAML parser, JSON Schema & semantic validator.

pub mod ast;
pub mod runtime;
pub mod schema;
pub mod validator;

pub use ast::{ActionSpec, ConnectionSpec, StepSpec, TriggerSpec, WorkflowSpec};
pub use runtime::{execute_workflow, ExecutionOptions, RunReport, StepLog};
pub use schema::generate_schema_json;
pub use validator::{validate_workflow, workflow_sha256, ValidationError, ValidationReport};

/// Parse un document YAML brut en [`WorkflowSpec`] strict (`deny_unknown_fields`).
pub fn parse_workflow_yaml(yaml: &str) -> Result<WorkflowSpec, String> {
    serde_yaml_ng::from_str::<WorkflowSpec>(yaml).map_err(|e| format!("Erreur parsing YAML WFL : {}", e))
}
