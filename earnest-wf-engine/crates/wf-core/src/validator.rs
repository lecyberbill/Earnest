use std::collections::BTreeSet;
use std::fmt;

use sha2::{Digest, Sha256};

use crate::ast::{ActionSpec, StepSpec, TriggerSpec, WorkflowSpec};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ValidationError {
    pub code: &'static str,
    pub path: String,
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.code, self.path, self.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ValidationReport {
    pub valid: bool,
    pub sha256: String,
    pub step_count: usize,
    pub errors: Vec<ValidationError>,
}

const RESERVED_NAMESPACES: &[&str] = &["inputs", "vars", "env", "keyring", "error"];

/// Calcule l'empreinte SHA-256 déterministe d'un [`WorkflowSpec`] sérialisé en JSON canonique.
fn canonicalize_json(v: serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(map) => {
            let sorted: std::collections::BTreeMap<String, serde_json::Value> = map
                .into_iter()
                .map(|(k, val)| (k, canonicalize_json(val)))
                .collect();
            let out: serde_json::Map<String, serde_json::Value> = sorted.into_iter().collect();
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(canonicalize_json).collect())
        }
        other => other,
    }
}

pub fn workflow_sha256(spec: &WorkflowSpec) -> String {
    let val = serde_json::to_value(spec)
        .map(canonicalize_json)
        .unwrap_or(serde_json::Value::Null);
    let json = serde_json::to_string(&val).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Valide statiquement et sémantiquement un [`WorkflowSpec`].
pub fn validate_workflow(spec: &WorkflowSpec) -> ValidationReport {
    let mut errors = Vec::new();

    if spec.version.trim().is_empty() {
        errors.push(ValidationError {
            code: "EMPTY_VERSION",
            path: "version".into(),
            message: "La version du workflow ne peut pas être vide".into(),
        });
    }
    if spec.name.trim().is_empty() {
        errors.push(ValidationError {
            code: "EMPTY_NAME",
            path: "name".into(),
            message: "Le nom du workflow ne peut pas être vide".into(),
        });
    }
    if spec.steps.is_empty() {
        errors.push(ValidationError {
            code: "EMPTY_STEPS",
            path: "steps".into(),
            message: "Un workflow doit contenir au moins une étape".into(),
        });
    }

    for (idx, trig) in spec.triggers.iter().enumerate() {
        match trig {
            TriggerSpec::VfsWatch { conn, .. } | TriggerSpec::MailWatch { conn, .. }
                if !spec.connections.contains_key(conn) =>
            {
                errors.push(ValidationError {
                    code: "UNKNOWN_CONNECTION",
                    path: format!("triggers[{}].conn", idx),
                    message: format!("Connexion '{}' inexistante dans `connections`", conn),
                });
            }
            TriggerSpec::Cron { expr, .. } if expr.trim().is_empty() => {
                errors.push(ValidationError {
                    code: "INVALID_CRON",
                    path: format!("triggers[{}].expr", idx),
                    message: "Expression cron vide".into(),
                });
            }
            _ => {}
        }
    }

    let mut seen_ids = BTreeSet::new();
    let mut step_count = 0usize;
    validate_steps(spec, &spec.steps, "steps", &mut seen_ids, &mut step_count, &mut errors);

    ValidationReport {
        valid: errors.is_empty(),
        sha256: workflow_sha256(spec),
        step_count,
        errors,
    }
}

fn validate_steps(
    spec: &WorkflowSpec,
    steps: &[StepSpec],
    prefix: &str,
    seen_ids: &mut BTreeSet<String>,
    step_count: &mut usize,
    errors: &mut Vec<ValidationError>,
) {
    for (idx, step) in steps.iter().enumerate() {
        *step_count += 1;
        let path = format!("{}[{}]", prefix, idx);

        if step.id.trim().is_empty() {
            errors.push(ValidationError {
                code: "EMPTY_STEP_ID",
                path: format!("{}.id", path),
                message: "L'identifiant d'étape ne peut pas être vide".into(),
            });
        } else if !seen_ids.insert(step.id.clone()) {
            errors.push(ValidationError {
                code: "DUPLICATE_STEP_ID",
                path: format!("{}.id", path),
                message: format!("Identifiant d'étape dupliqué : '{}'", step.id),
            });
        }

        if let Some(ref out) = step.output {
            let trimmed = out.trim();
            if trimmed.is_empty() {
                errors.push(ValidationError {
                    code: "EMPTY_OUTPUT_NAME",
                    path: format!("{}.output", path),
                    message: "Le nom de variable `output` ne peut pas être vide".into(),
                });
            } else if RESERVED_NAMESPACES.contains(&trimmed) {
                errors.push(ValidationError {
                    code: "RESERVED_OUTPUT_NAME",
                    path: format!("{}.output", path),
                    message: format!("Le nom '{}' est un espace réservé", trimmed),
                });
            }
        }

        if let Some(ref retry) = step.retry {
            if retry.max_attempts == 0 {
                errors.push(ValidationError {
                    code: "INVALID_RETRY",
                    path: format!("{}.retry.max_attempts", path),
                    message: "max_attempts doit être >= 1".into(),
                });
            }
        }

        validate_action(spec, &step.action, &path, seen_ids, step_count, errors);

        if !step.on_error.is_empty() {
            validate_steps(
                spec,
                &step.on_error,
                &format!("{}.on_error", path),
                seen_ids,
                step_count,
                errors,
            );
        }
    }
}

fn check_conn(spec: &WorkflowSpec, conn: &str, path: String, errors: &mut Vec<ValidationError>) {
    if !spec.connections.contains_key(conn) {
        errors.push(ValidationError {
            code: "UNKNOWN_CONNECTION",
            path,
            message: format!("Connexion '{}' introuvable dans `connections`", conn),
        });
    }
}

fn validate_action(
    spec: &WorkflowSpec,
    action: &ActionSpec,
    path: &str,
    seen_ids: &mut BTreeSet<String>,
    step_count: &mut usize,
    errors: &mut Vec<ValidationError>,
) {
    match action {
        ActionSpec::VfsList { params } => {
            check_conn(spec, &params.conn, format!("{}.params.conn", path), errors)
        }
        ActionSpec::VfsRead { params } => {
            check_conn(spec, &params.conn, format!("{}.params.conn", path), errors)
        }
        ActionSpec::VfsWrite { params } => {
            check_conn(spec, &params.conn, format!("{}.params.conn", path), errors)
        }
        ActionSpec::VfsDelete { params } => {
            check_conn(spec, &params.conn, format!("{}.params.conn", path), errors)
        }
        ActionSpec::VfsStat { params } => {
            check_conn(spec, &params.conn, format!("{}.params.conn", path), errors)
        }
        ActionSpec::VfsAppend { params } => {
            check_conn(spec, &params.conn, format!("{}.params.conn", path), errors)
        }
        ActionSpec::VfsMkdir { params } | ActionSpec::VfsExists { params } => {
            check_conn(spec, &params.conn, format!("{}.params.conn", path), errors)
        }
        ActionSpec::VfsMove { params } => {
            check_conn(spec, &params.src.conn, format!("{}.params.src.conn", path), errors);
            check_conn(spec, &params.dst.conn, format!("{}.params.dst.conn", path), errors);
        }
        ActionSpec::VfsTransfer { params } => {
            check_conn(spec, &params.src.conn, format!("{}.params.src.conn", path), errors);
            check_conn(spec, &params.dst.conn, format!("{}.params.dst.conn", path), errors);
        }
        ActionSpec::MailSend { params } => {
            check_conn(spec, &params.conn, format!("{}.params.conn", path), errors);
            for (i, att) in params.attachments.iter().enumerate() {
                check_conn(
                    spec,
                    &att.conn,
                    format!("{}.params.attachments[{}].conn", path, i),
                    errors,
                );
            }
        }
        ActionSpec::MailFetch { params } => {
            check_conn(spec, &params.conn, format!("{}.params.conn", path), errors)
        }
        ActionSpec::SysHttp { params } => {
            if let Some(ref c) = params.conn {
                check_conn(spec, c, format!("{}.params.conn", path), errors);
            }
        }
        ActionSpec::FlowIf {
            condition,
            then,
            else_branch,
        } => {
            if condition.trim().is_empty() {
                errors.push(ValidationError {
                    code: "EMPTY_CONDITION",
                    path: format!("{}.condition", path),
                    message: "La condition de `flow.if` ne peut pas être vide".into(),
                });
            }
            validate_steps(spec, then, &format!("{}.then", path), seen_ids, step_count, errors);
            validate_steps(
                spec,
                else_branch,
                &format!("{}.else", path),
                seen_ids,
                step_count,
                errors,
            );
        }
        ActionSpec::FlowEach {
            concurrency,
            steps,
            item_var,
            ..
        } => {
            if *concurrency == 0 {
                errors.push(ValidationError {
                    code: "INVALID_CONCURRENCY",
                    path: format!("{}.concurrency", path),
                    message: "concurrency doit être >= 1".into(),
                });
            }
            if item_var.trim().is_empty() {
                errors.push(ValidationError {
                    code: "EMPTY_ITEM_VAR",
                    path: format!("{}.as", path),
                    message: "La variable d'itération `as` ne peut pas être vide".into(),
                });
            }
            validate_steps(spec, steps, &format!("{}.steps", path), seen_ids, step_count, errors);
        }
        ActionSpec::FlowWhile {
            max_iterations,
            steps,
            ..
        } => {
            if *max_iterations == 0 {
                errors.push(ValidationError {
                    code: "INVALID_MAX_ITERATIONS",
                    path: format!("{}.max_iterations", path),
                    message: "max_iterations doit être >= 1".into(),
                });
            }
            validate_steps(spec, steps, &format!("{}.steps", path), seen_ids, step_count, errors);
        }
        ActionSpec::FlowSwitch { cases, default, .. } => {
            for (k, branch) in cases {
                validate_steps(spec, branch, &format!("{}.cases.{}", path, k), seen_ids, step_count, errors);
            }
            validate_steps(spec, default, &format!("{}.default", path), seen_ids, step_count, errors);
        }
        _ => {}
    }
}
