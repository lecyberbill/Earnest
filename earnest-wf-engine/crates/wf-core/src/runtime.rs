use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::ast::{ActionSpec, ConnectionSpec, ReadEncoding, StepSpec, WorkflowSpec};
use crate::validator::validate_workflow;

#[derive(Debug, Clone)]
pub struct ExecutionOptions {
    pub inputs: HashMap<String, Value>,
    pub dry_run: bool,
    pub max_each_items: usize,
    pub secrets: HashMap<String, String>,
}

impl Default for ExecutionOptions {
    fn default() -> Self {
        Self {
            inputs: HashMap::new(),
            dry_run: false,
            max_each_items: 1_000,
            secrets: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepLog {
    pub step_id: String,
    pub status: String,
    pub attempt: u32,
    pub output: Option<Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    pub workflow_name: String,
    pub workflow_sha256: String,
    pub dry_run: bool,
    pub success: bool,
    pub logs: Vec<StepLog>,
    pub error: Option<String>,
}

struct RuntimeCtx<'a> {
    spec: &'a WorkflowSpec,
    inputs: HashMap<String, Value>,
    vars: HashMap<String, Value>,
    outputs: HashMap<String, Value>,
    secrets: HashMap<String, String>,
    dry_run: bool,
    dry_vfs_files: HashMap<PathBuf, Vec<u8>>,
    dry_vfs_deleted: HashSet<PathBuf>,
    max_each_items: usize,
    logs: Vec<StepLog>,
}

impl<'a> RuntimeCtx<'a> {
    fn sanitize(&self, text: &str) -> String {
        let mut s = log_sanitizer::sanitize_text(text);
        for v in self.secrets.values() {
            if !v.is_empty() && v.len() >= 4 {
                s = s.replace(v, "[REDACTED:SECRET]");
            }
        }
        s
    }

    fn resolve_path_expr(&self, expr: &str) -> Option<Value> {
        let expr = expr.trim();
        let clean = expr
            .strip_suffix("| length")
            .or_else(|| expr.strip_suffix(".len()"))
            .map(str::trim);
        if let Some(inner) = clean {
            let val = self.resolve_path_expr(inner)?;
            let len = match val {
                Value::Array(a) => a.len(),
                Value::String(s) => s.len(),
                Value::Object(o) => o.len(),
                _ => 0,
            };
            return Some(Value::from(len as u64));
        }

        let mut parts = expr.split('.');
        let head = parts.next()?;
        let mut cur = match head {
            "inputs" => {
                let k = parts.next()?;
                self.inputs.get(k)?.clone()
            }
            "vars" => {
                let k = parts.next()?;
                self.vars.get(k)?.clone()
            }
            "env" => {
                let k = parts.next()?;
                std::env::var(k).ok().map(Value::String)?
            }
            "keyring" => {
                let k = parts.next()?;
                self.secrets.get(k).cloned().map(Value::String)?
            }
            other => self
                .outputs
                .get(other)
                .or_else(|| self.vars.get(other))
                .or_else(|| self.inputs.get(other))?
                .clone(),
        };

        for p in parts {
            cur = match cur {
                Value::Object(map) => map.get(p)?.clone(),
                Value::Array(arr) => {
                    let idx: usize = p.parse().ok()?;
                    arr.get(idx)?.clone()
                }
                _ => return None,
            };
        }
        Some(cur)
    }

    fn render_str(&self, template: &str) -> String {
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            if let Some(end) = after.find("}}") {
                let expr = after[..end].trim();
                let rendered = match self.resolve_path_expr(expr) {
                    Some(Value::String(s)) => s,
                    Some(Value::Number(n)) => n.to_string(),
                    Some(Value::Bool(b)) => b.to_string(),
                    Some(Value::Null) | None => String::new(),
                    Some(other) => other.to_string(),
                };
                out.push_str(&rendered);
                rest = &after[end + 2..];
            } else {
                out.push_str("{{");
                rest = after;
            }
        }
        out.push_str(rest);
        out
    }

    fn eval_condition(&self, cond: &str) -> bool {
        let c = cond.trim();
        if c == "true" {
            return true;
        }
        if c == "false" {
            return false;
        }
        for op in [">=", "<=", "==", "!=", ">", "<"] {
            if let Some((lhs, rhs)) = c.split_once(op) {
                let lv = self.eval_operand(lhs.trim());
                let rv = self.eval_operand(rhs.trim());
                return compare_values(&lv, &rv, op);
            }
        }
        match self.resolve_path_expr(c) {
            Some(Value::Bool(b)) => b,
            Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
            Some(Value::String(s)) => !s.is_empty(),
            Some(Value::Array(a)) => !a.is_empty(),
            _ => false,
        }
    }

    fn eval_operand(&self, raw: &str) -> Value {
        if (raw.starts_with('"') && raw.ends_with('"'))
            || (raw.starts_with('\'') && raw.ends_with('\''))
        {
            return Value::String(raw[1..raw.len() - 1].to_string());
        }
        if raw == "true" {
            return Value::Bool(true);
        }
        if raw == "false" {
            return Value::Bool(false);
        }
        if let Ok(i) = raw.parse::<i64>() {
            return Value::from(i);
        }
        if let Ok(f) = raw.parse::<f64>() {
            return json!(f);
        }
        self.resolve_path_expr(raw).unwrap_or(Value::Null)
    }

    fn local_root(&self, conn_name: &str) -> Result<PathBuf, String> {
        match self.spec.connections.get(conn_name) {
            Some(ConnectionSpec::Local { root }) => {
                let rendered = self.render_str(root);
                Ok(PathBuf::from(rendered))
            }
            Some(_) => Err(format!(
                "Driver de '{}' non supporté par le runtime local (Étape 2)",
                conn_name
            )),
            None => Err(format!("Connexion '{}' introuvable", conn_name)),
        }
    }

    fn safe_local_path(&self, conn_name: &str, rel_raw: &str) -> Result<PathBuf, String> {
        let root = self.local_root(conn_name)?;
        let rendered = self.render_str(rel_raw);
        let rel = rendered.trim().trim_start_matches(['/', '\\']);
        let p = Path::new(rel);
        if tool_guard::path::has_traversal(p) {
            return Err(format!("Path traversal refusé : '{}'", rendered));
        }
        if tool_guard::path::is_system_sensitive(p) {
            return Err(format!("Chemin système refusé : '{}'", rendered));
        }
        if rel.is_empty() {
            Ok(root)
        } else {
            Ok(root.join(tool_guard::path::normalize(p)))
        }
    }
}

fn compare_values(l: &Value, r: &Value, op: &str) -> bool {
    if let (Some(lf), Some(rf)) = (l.as_f64(), r.as_f64()) {
        return match op {
            "==" => (lf - rf).abs() < f64::EPSILON,
            "!=" => (lf - rf).abs() >= f64::EPSILON,
            ">" => lf > rf,
            "<" => lf < rf,
            ">=" => lf >= rf,
            "<=" => lf <= rf,
            _ => false,
        };
    }
    match op {
        "==" => l == r,
        "!=" => l != r,
        _ => false,
    }
}

/// Exécute un [`WorkflowSpec`] de façon déterministe (avec support `dry_run`, `retry`, `on_error`).
pub fn execute_workflow(spec: &WorkflowSpec, opts: ExecutionOptions) -> Result<RunReport, String> {
    let validation = validate_workflow(spec);
    if !validation.valid {
        let msgs: Vec<String> = validation.errors.iter().map(|e| e.to_string()).collect();
        return Err(format!("Workflow invalide :\n{}", msgs.join("\n")));
    }

    let mut inputs = HashMap::new();
    for (name, param) in &spec.inputs {
        if let Some(val) = opts.inputs.get(name) {
            inputs.insert(name.clone(), val.clone());
        } else if let Some(ref def) = param.default {
            inputs.insert(name.clone(), def.clone());
        } else if param.required {
            return Err(format!("Paramètre d'entrée requis manquant : '{}'", name));
        }
    }

    let mut ctx = RuntimeCtx {
        spec,
        inputs,
        vars: spec.vars.clone(),
        outputs: HashMap::new(),
        secrets: opts.secrets,
        dry_run: opts.dry_run,
        dry_vfs_files: HashMap::new(),
        dry_vfs_deleted: HashSet::new(),
        max_each_items: opts.max_each_items.max(1),
        logs: Vec::new(),
    };

    let res = exec_steps(&mut ctx, &spec.steps);
    Ok(RunReport {
        workflow_name: spec.name.clone(),
        workflow_sha256: validation.sha256,
        dry_run: ctx.dry_run,
        success: res.is_ok(),
        logs: ctx.logs,
        error: res.err(),
    })
}

fn exec_steps(ctx: &mut RuntimeCtx<'_>, steps: &[StepSpec]) -> Result<(), String> {
    for step in steps {
        exec_step(ctx, step)?;
    }
    Ok(())
}

fn exec_step(ctx: &mut RuntimeCtx<'_>, step: &StepSpec) -> Result<(), String> {
    let max_attempts = step.retry.as_ref().map(|r| r.max_attempts.max(1)).unwrap_or(1);
    let mut last_err = String::new();

    for attempt in 1..=max_attempts {
        match exec_action(ctx, step) {
            Ok(out_val) => {
                if let Some(ref out_name) = step.output {
                    if let Some(ref v) = out_val {
                        ctx.outputs.insert(out_name.clone(), v.clone());
                    }
                }
                let sanitized_out = out_val.map(|v| {
                    let s = ctx.sanitize(&v.to_string());
                    serde_json::from_str(&s).unwrap_or(Value::String(s))
                });
                ctx.logs.push(StepLog {
                    step_id: step.id.clone(),
                    status: if ctx.dry_run { "dry_run" } else { "success" }.to_string(),
                    attempt,
                    output: sanitized_out,
                    error: None,
                });
                return Ok(());
            }
            Err(err) => {
                last_err = ctx.sanitize(&err);
                if attempt < max_attempts {
                    if let Some(ref retry) = step.retry {
                        if !ctx.dry_run && retry.backoff_ms > 0 {
                            let mult = if retry.exponential { 1u64 << (attempt - 1).min(6) } else { 1 };
                            std::thread::sleep(Duration::from_millis(retry.backoff_ms.saturating_mul(mult)));
                        }
                    }
                }
            }
        }
    }

    ctx.logs.push(StepLog {
        step_id: step.id.clone(),
        status: "failed".to_string(),
        attempt: max_attempts,
        output: None,
        error: Some(last_err.clone()),
    });

    if !step.on_error.is_empty() {
        ctx.vars.insert("error".to_string(), json!({ "message": last_err }));
        exec_steps(ctx, &step.on_error)?;
        return Ok(());
    }

    Err(last_err)
}

fn exec_action(ctx: &mut RuntimeCtx<'_>, step: &StepSpec) -> Result<Option<Value>, String> {
    match &step.action {
        ActionSpec::FlowSet { params } => {
            let val = match &params.value {
                Value::String(s) => Value::String(ctx.render_str(s)),
                other => other.clone(),
            };
            ctx.vars.insert(params.var.clone(), val.clone());
            Ok(Some(val))
        }
        ActionSpec::FlowSleep { params } => {
            if !ctx.dry_run && params.duration_ms > 0 {
                std::thread::sleep(Duration::from_millis(params.duration_ms));
            }
            Ok(Some(json!({ "slept_ms": params.duration_ms })))
        }
        ActionSpec::FlowFail { params } => {
            Err(format!("[{}] {}", params.code, ctx.render_str(&params.message)))
        }
        ActionSpec::SysLog { params } => {
            let msg = ctx.sanitize(&ctx.render_str(&params.message));
            Ok(Some(json!({ "level": format!("{:?}", params.level), "message": msg })))
        }
        ActionSpec::FlowIf {
            condition,
            then,
            else_branch,
        } => {
            let cond = ctx.eval_condition(condition);
            if cond {
                exec_steps(ctx, then)?;
            } else {
                exec_steps(ctx, else_branch)?;
            }
            Ok(Some(json!({ "branch": if cond { "then" } else { "else" } })))
        }
        ActionSpec::FlowEach {
            items,
            item_var,
            steps,
            ..
        } => {
            let expr = items
                .trim()
                .trim_start_matches("{{")
                .trim_end_matches("}}")
                .trim();
            let list = match ctx.resolve_path_expr(expr) {
                Some(Value::Array(arr)) => arr,
                _ => return Err(format!("`flow.each` attend un tableau pour '{}'", items)),
            };
            if list.len() > ctx.max_each_items {
                return Err(format!(
                    "`flow.each` dépasse le plafond {} éléments ({})",
                    ctx.max_each_items,
                    list.len()
                ));
            }
            let prev = ctx.vars.get(item_var).cloned();
            for item in list {
                ctx.vars.insert(item_var.clone(), item);
                exec_steps(ctx, steps)?;
            }
            match prev {
                Some(v) => {
                    ctx.vars.insert(item_var.clone(), v);
                }
                None => {
                    ctx.vars.remove(item_var);
                }
            }
            Ok(Some(json!({ "status": "completed" })))
        }
        ActionSpec::VfsList { params } => {
            let dir = ctx.safe_local_path(&params.conn, &params.path)?;
            let mut entries = Vec::new();
            let rd = fs::read_dir(&dir).map_err(|e| format!("vfs.list '{}': {}", dir.display(), e))?;
            for entry in rd.flatten() {
                let p = entry.path();
                if p.is_file() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if let Some(ref glob) = params.glob {
                        if let Some(suffix) = glob.strip_prefix('*') {
                            if !name.ends_with(suffix) {
                                continue;
                            }
                        } else if &name != glob {
                            continue;
                        }
                    }
                    entries.push(json!({
                        "name": name,
                        "path": name,
                    }));
                }
            }
            Ok(Some(json!({ "data": entries })))
        }
        ActionSpec::VfsRead { params } => {
            let path = ctx.safe_local_path(&params.conn, &params.path)?;
            let bytes = if ctx.dry_run {
                if ctx.dry_vfs_deleted.contains(&path) {
                    return Err(format!("vfs.read '{}': fichier supprimé en dry_run", path.display()));
                }
                if let Some(mem) = ctx.dry_vfs_files.get(&path) {
                    mem.clone()
                } else {
                    fs::read(&path).map_err(|e| format!("vfs.read '{}': {}", path.display(), e))?
                }
            } else {
                fs::read(&path).map_err(|e| format!("vfs.read '{}': {}", path.display(), e))?
            };
            match params.encoding {
                ReadEncoding::Utf8 => {
                    let text = String::from_utf8(bytes).map_err(|e| format!("UTF-8 invalide: {}", e))?;
                    Ok(Some(json!({ "content": text })))
                }
                ReadEncoding::Base64 => Ok(Some(json!({ "bytes_len": bytes.len() }))),
            }
        }
        ActionSpec::VfsWrite { params } => {
            let path = ctx.safe_local_path(&params.conn, &params.path)?;
            let content = ctx.render_str(&params.content);
            if ctx.dry_run {
                ctx.dry_vfs_deleted.remove(&path);
                ctx.dry_vfs_files.insert(path, content.as_bytes().to_vec());
            } else {
                if params.create_dirs {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                }
                fs::write(&path, &content).map_err(|e| format!("vfs.write '{}': {}", path.display(), e))?;
            }
            Ok(Some(json!({ "bytes": content.len(), "dry_run": ctx.dry_run })))
        }
        ActionSpec::VfsTransfer { params } => {
            let src = ctx.safe_local_path(&params.src.conn, &params.src.path)?;
            let dst = ctx.safe_local_path(&params.dst.conn, &params.dst.path)?;
            let dst_exists = if ctx.dry_run {
                !ctx.dry_vfs_deleted.contains(&dst) && (ctx.dry_vfs_files.contains_key(&dst) || dst.exists())
            } else {
                dst.exists()
            };
            if !params.overwrite && dst_exists {
                return Err(format!("Destination existante (overwrite=false): {}", dst.display()));
            }
            if ctx.dry_run {
                let bytes = if ctx.dry_vfs_deleted.contains(&src) {
                    return Err(format!("vfs.transfer: source supprimée en dry_run '{}'", src.display()));
                } else if let Some(mem) = ctx.dry_vfs_files.get(&src) {
                    mem.clone()
                } else {
                    fs::read(&src).map_err(|e| format!("vfs.transfer '{}': {}", src.display(), e))?
                };
                ctx.dry_vfs_deleted.remove(&dst);
                ctx.dry_vfs_files.insert(dst.clone(), bytes);
                return Ok(Some(json!({ "dry_run": true, "src": src.display().to_string(), "dst": dst.display().to_string() })));
            }
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let bytes = fs::copy(&src, &dst).map_err(|e| format!("vfs.transfer: {}", e))?;
            Ok(Some(json!({ "bytes": bytes })))
        }
        ActionSpec::VfsDelete { params } => {
            let path = ctx.safe_local_path(&params.conn, &params.path)?;
            if ctx.dry_run {
                ctx.dry_vfs_files.remove(&path);
                ctx.dry_vfs_deleted.insert(path);
                return Ok(Some(json!({ "deleted": true, "dry_run": true })));
            }
            if path.exists() {
                fs::remove_file(&path).map_err(|e| format!("vfs.delete '{}': {}", path.display(), e))?;
            }
            Ok(Some(json!({ "deleted": true })))
        }
        ActionSpec::VfsStat { params } => {
            let path = ctx.safe_local_path(&params.conn, &params.path)?;
            if ctx.dry_run {
                if ctx.dry_vfs_deleted.contains(&path) {
                    return Err(format!("vfs.stat '{}': fichier supprimé en dry_run", path.display()));
                }
                if let Some(data) = ctx.dry_vfs_files.get(&path) {
                    let hash = if params.compute_sha256 {
                        let mut h = Sha256::new();
                        h.update(data);
                        Some(h.finalize().iter().map(|b| format!("{:02x}", b)).collect::<String>())
                    } else {
                        None
                    };
                    return Ok(Some(json!({ "size": data.len(), "is_file": true, "sha256": hash })));
                }
            }
            let meta = fs::metadata(&path).map_err(|e| format!("vfs.stat '{}': {}", path.display(), e))?;
            let hash = if params.compute_sha256 && meta.is_file() {
                let data = fs::read(&path).map_err(|e| e.to_string())?;
                let mut h = Sha256::new();
                h.update(&data);
                Some(h.finalize().iter().map(|b| format!("{:02x}", b)).collect::<String>())
            } else {
                None
            };
            Ok(Some(json!({ "size": meta.len(), "is_file": meta.is_file(), "sha256": hash })))
        }
        _ => Err("Action non implémentée dans le runtime local (Étape 2)".to_string()),
    }
}
