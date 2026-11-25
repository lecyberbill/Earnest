//! `wf-core` — WFL (Workflow Language) AST, YAML parser, JSON Schema & semantic validator.

pub mod ast;
pub mod runtime;
pub mod schema;
pub mod store;
pub mod validator;

pub use ast::{ActionSpec, ConnectionSpec, StepSpec, TriggerSpec, WorkflowSpec};
pub use cron_pulse;
pub use runtime::{execute_workflow, DebugSession, ExecutionOptions, RunReport, StepLog};
pub use schema::generate_schema_json;
pub use store::{RunDetails, RunSummary, WfStore, WorkflowRecord};
pub use validator::{validate_workflow, workflow_sha256, ValidationError, ValidationReport};

/// Parse un document YAML brut en [`WorkflowSpec`] strict (`deny_unknown_fields`).
pub fn parse_workflow_yaml(yaml: &str) -> Result<WorkflowSpec, String> {
    serde_yaml_ng::from_str::<WorkflowSpec>(yaml).map_err(|e| format!("Erreur parsing YAML WFL : {}", e))
}

/// Sérialise un [`WorkflowSpec`] en YAML WFL.
pub fn serialize_workflow_yaml(spec: &WorkflowSpec) -> Result<String, String> {
    serde_yaml_ng::to_string(spec).map_err(|e| format!("Erreur sérialisation YAML WFL : {}", e))
}

/// Parse une expression cron WFL (5 champs POSIX/macros, ou 6 champs avec secondes `0..=59` en tête).
pub fn parse_wfl_cron(expr: &str) -> Result<cron_pulse::CronSchedule, cron_pulse::CronError> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() == 6 {
        if let Ok(sec) = parts[0].parse::<u32>() {
            if sec <= 59 {
                return cron_pulse::CronSchedule::parse(&parts[1..].join(" "));
            }
        }
    }
    cron_pulse::CronSchedule::parse(expr)
}
