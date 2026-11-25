use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
        let stages = split_pipes_outside_quotes(expr.trim());
        let head_expr = stages.first()?.trim();
        let mut cur = self.resolve_base_expr(head_expr)?;
        for pipe in stages.into_iter().skip(1) {
            cur = apply_pipe(cur, pipe.trim())?;
        }
        Some(cur)
    }

    fn resolve_base_expr(&self, expr: &str) -> Option<Value> {
        let expr = expr.trim();
        if let Some(inner) = expr.strip_prefix("now(").and_then(|s| s.strip_suffix(')')) {
            let fmt = inner.trim().trim_matches(|c| c == '\'' || c == '"');
            let fmt = if fmt.is_empty() { "%Y-%m-%d %H:%M:%S" } else { fmt };
            return Some(Value::String(chrono::Local::now().format(fmt).to_string()));
        }
        if expr == "now" {
            return Some(Value::String(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()));
        }
        if let Some(inner) = expr.strip_suffix(".len()").map(str::trim) {
            let val = self.resolve_base_expr(inner)?;
            return Some(Value::from(value_len(&val) as u64));
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
                self.vars.get(k).or_else(|| self.outputs.get(k))?.clone()
            }
            "outputs" => {
                let k = parts.next()?;
                self.outputs.get(k)?.clone()
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
        while !rest.is_empty() {
            let next_brace = rest.find("{{");
            let next_dollar = rest.find("${");
            let (start, open, close) = match (next_brace, next_dollar) {
                (Some(a), Some(b)) if a <= b => (a, "{{", "}}"),
                (Some(_), Some(b)) => (b, "${", "}"),
                (Some(a), None) => (a, "{{", "}}"),
                (None, Some(b)) => (b, "${", "}"),
                (None, None) => {
                    out.push_str(rest);
                    break;
                }
            };
            out.push_str(&rest[..start]);
            let after = &rest[start + open.len()..];
            if let Some(end) = after.find(close) {
                let expr = after[..end].trim();
                let rendered = match self.resolve_path_expr(expr) {
                    Some(Value::String(s)) => s,
                    Some(Value::Number(n)) => n.to_string(),
                    Some(Value::Bool(b)) => b.to_string(),
                    Some(Value::Null) | None => String::new(),
                    Some(other) => other.to_string(),
                };
                out.push_str(&rendered);
                rest = &after[end + close.len()..];
            } else {
                out.push_str(open);
                rest = after;
            }
        }
        out
    }

    fn render_str_checked(&self, template: &str) -> Result<String, String> {
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while !rest.is_empty() {
            let next_brace = rest.find("{{");
            let next_dollar = rest.find("${");
            let (start, open, close) = match (next_brace, next_dollar) {
                (Some(a), Some(b)) if a <= b => (a, "{{", "}}"),
                (Some(_), Some(b)) => (b, "${", "}"),
                (Some(a), None) => (a, "{{", "}}"),
                (None, Some(b)) => (b, "${", "}"),
                (None, None) => {
                    out.push_str(rest);
                    break;
                }
            };
            out.push_str(&rest[..start]);
            let after = &rest[start + open.len()..];
            if let Some(end) = after.find(close) {
                let expr = after[..end].trim();
                let resolved = self.resolve_path_expr(expr).ok_or_else(|| {
                    format!("Variable ou expression introuvable : '{}{}{}'", open, expr, close)
                })?;
                let rendered = match resolved {
                    Value::String(s) => s,
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Null => String::new(),
                    other => other.to_string(),
                };
                out.push_str(&rendered);
                rest = &after[end + close.len()..];
            } else {
                out.push_str(open);
                rest = after;
            }
        }
        Ok(out)
    }

    fn eval_condition(&self, cond: &str) -> bool {
        let c = unwrap_outer_parens(cond.trim());
        if c == "true" {
            return true;
        }
        if c == "false" {
            return false;
        }
        if let Some((lhs, rhs)) = split_logical_op(c, "||") {
            return self.eval_condition(lhs) || self.eval_condition(rhs);
        }
        if let Some((lhs, rhs)) = split_logical_op(c, "&&") {
            return self.eval_condition(lhs) && self.eval_condition(rhs);
        }
        if let Some(inner) = c.strip_prefix('!') {
            return !self.eval_condition(inner);
        }
        for op in [" contains ", " starts_with ", " ends_with ", " matches ", " in "] {
            if let Some((lhs, rhs)) = split_outside_quotes(c, op) {
                let lv = self.eval_operand(lhs.trim());
                let rv = self.eval_operand(rhs.trim());
                return eval_predicate(&lv, &rv, op.trim());
            }
        }
        for op in [">=", "<=", "==", "!=", ">", "<"] {
            if let Some((lhs, rhs)) = split_outside_quotes(c, op) {
                let lv = self.eval_operand(lhs.trim());
                let rv = self.eval_operand(rhs.trim());
                return compare_values(&lv, &rv, op);
            }
        }
        let expr = unwrap_template_expr(c);
        match self.resolve_path_expr(expr) {
            Some(Value::Bool(b)) => b,
            Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
            Some(Value::String(s)) => !s.is_empty(),
            Some(Value::Array(a)) => !a.is_empty(),
            Some(Value::Object(o)) => !o.is_empty(),
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
        let expr = unwrap_template_expr(raw);
        self.resolve_path_expr(expr).unwrap_or(Value::Null)
    }

    fn local_root(&self, conn_name: &str) -> Result<PathBuf, String> {
        match self.spec.connections.get(conn_name) {
            Some(ConnectionSpec::Local { root }) => {
                let rendered = self.render_str_checked(root)?;
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
        let rendered = self.render_str_checked(rel_raw)?;
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

    let mut secrets = opts.secrets;
    if let Ok(vault) = chromatix_vault::TenantVault::from_env(chromatix_vault::resolve_vaults_dir()) {
        for tenant in ["default", spec.name.as_str()] {
            if let Ok(loaded) = vault.load_all(tenant) {
                for (k, v) in loaded {
                    secrets.entry(k).or_insert(v);
                }
            }
        }
    }

    let mut ctx = RuntimeCtx {
        spec,
        inputs,
        vars: spec.vars.clone(),
        outputs: HashMap::new(),
        secrets,
        dry_run: opts.dry_run,
        dry_vfs_files: HashMap::new(),
        dry_vfs_deleted: HashSet::new(),
        max_each_items: opts.max_each_items.max(1),
        logs: Vec::new(),
    };
    init_dynamic_vars(&mut ctx);

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

/// Session d'exécution pas à pas (pour le débugger interactif de `wf-ui`).
pub struct DebugSession {
    spec: WorkflowSpec,
    inputs: HashMap<String, Value>,
    vars: HashMap<String, Value>,
    outputs: HashMap<String, Value>,
    secrets: HashMap<String, String>,
    dry_run: bool,
    dry_vfs_files: HashMap<PathBuf, Vec<u8>>,
    dry_vfs_deleted: HashSet<PathBuf>,
    max_each_items: usize,
    logs: Vec<StepLog>,
    next_index: usize,
    finished: bool,
    error: Option<String>,
}

impl DebugSession {
    pub fn new(spec: WorkflowSpec, opts: ExecutionOptions) -> Result<Self, String> {
        let validation = validate_workflow(&spec);
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
        let vars = spec.vars.clone();
        Ok(Self {
            spec,
            inputs,
            vars,
            outputs: HashMap::new(),
            secrets: opts.secrets,
            dry_run: opts.dry_run,
            dry_vfs_files: HashMap::new(),
            dry_vfs_deleted: HashSet::new(),
            max_each_items: opts.max_each_items.max(1),
            logs: Vec::new(),
            next_index: 0,
            finished: false,
            error: None,
        })
    }

    pub fn current_step_id(&self) -> Option<&str> {
        if self.finished {
            None
        } else {
            self.spec.steps.get(self.next_index).map(|s| s.id.as_str())
        }
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn logs(&self) -> &[StepLog] {
        &self.logs
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Exécute l'étape suivante (`spec.steps[next_index]`) et retourne son dernier `StepLog`.
    pub fn step(&mut self) -> Option<StepLog> {
        if self.finished || self.next_index >= self.spec.steps.len() {
            self.finished = true;
            return None;
        }
        let step_spec = self.spec.steps[self.next_index].clone();
        let mut ctx = RuntimeCtx {
            spec: &self.spec,
            inputs: std::mem::take(&mut self.inputs),
            vars: std::mem::take(&mut self.vars),
            outputs: std::mem::take(&mut self.outputs),
            secrets: std::mem::take(&mut self.secrets),
            dry_run: self.dry_run,
            dry_vfs_files: std::mem::take(&mut self.dry_vfs_files),
            dry_vfs_deleted: std::mem::take(&mut self.dry_vfs_deleted),
            max_each_items: self.max_each_items,
            logs: std::mem::take(&mut self.logs),
        };
        let res = exec_step(&mut ctx, &step_spec);
        self.inputs = ctx.inputs;
        self.vars = ctx.vars;
        self.outputs = ctx.outputs;
        self.secrets = ctx.secrets;
        self.dry_vfs_files = ctx.dry_vfs_files;
        self.dry_vfs_deleted = ctx.dry_vfs_deleted;
        self.logs = ctx.logs;

        self.next_index += 1;
        if let Err(e) = res {
            self.error = Some(e);
            self.finished = true;
        } else if self.next_index >= self.spec.steps.len() {
            self.finished = true;
        }
        self.logs.last().cloned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopSignal {
    None,
    Continue,
    Break,
}

fn init_dynamic_vars(ctx: &mut RuntimeCtx<'_>) {
    let keys: Vec<String> = ctx.spec.vars.keys().cloned().collect();
    for k in keys {
        if let Some(Value::String(s)) = ctx.vars.get(&k).cloned() {
            let rendered = ctx.render_str(&s);
            let val = eval_arithmetic(&rendered).unwrap_or(Value::String(rendered));
            ctx.vars.insert(k, val);
        }
    }
}

fn exec_steps(ctx: &mut RuntimeCtx<'_>, steps: &[StepSpec]) -> Result<LoopSignal, String> {
    for step in steps {
        let sig = exec_step(ctx, step)?;
        if sig != LoopSignal::None {
            return Ok(sig);
        }
    }
    Ok(LoopSignal::None)
}

fn exec_step(ctx: &mut RuntimeCtx<'_>, step: &StepSpec) -> Result<LoopSignal, String> {
    let max_attempts = step.retry.as_ref().map(|r| r.max_attempts.max(1)).unwrap_or(1);
    let mut last_err = String::new();

    for attempt in 1..=max_attempts {
        match exec_action(ctx, step) {
            Ok((out_val, sig)) => {
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
                return Ok(sig);
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
        return exec_steps(ctx, &step.on_error);
    }

    Err(last_err)
}

fn exec_action(ctx: &mut RuntimeCtx<'_>, step: &StepSpec) -> Result<(Option<Value>, LoopSignal), String> {
    match &step.action {
        ActionSpec::FlowBreak {} => Ok((Some(json!({ "loop": "break" })), LoopSignal::Break)),
        ActionSpec::FlowContinue {} => Ok((Some(json!({ "loop": "continue" })), LoopSignal::Continue)),
        ActionSpec::FlowIf {
            condition,
            then,
            else_branch,
        } => {
            let cond = ctx.eval_condition(condition);
            let sig = if cond {
                exec_steps(ctx, then)?
            } else {
                exec_steps(ctx, else_branch)?
            };
            Ok((Some(json!({ "branch": if cond { "then" } else { "else" } })), sig))
        }
        ActionSpec::FlowSwitch {
            value,
            cases,
            default,
        } => {
            let key = ctx.render_str(value);
            let branch = cases.get(&key).unwrap_or(default);
            let sig = exec_steps(ctx, branch)?;
            Ok((Some(json!({ "matched": key })), sig))
        }
        ActionSpec::FlowEach {
            items,
            item_var,
            steps,
            ..
        } => {
            let expr = unwrap_template_expr(items.trim());
            let list = match ctx.resolve_path_expr(expr) {
                Some(Value::Array(arr)) => arr,
                Some(Value::Object(map)) => match map.get("data") {
                    Some(Value::Array(arr)) => arr.clone(),
                    _ => return Err(format!("`flow.each` attend un tableau pour '{}'", items)),
                },
                _ => return Err(format!("`flow.each` attend un tableau pour '{}'", items)),
            };
            if list.len() > ctx.max_each_items {
                return Err(format!(
                    "`flow.each` dépasse le plafond {} éléments ({})",
                    ctx.max_each_items,
                    list.len()
                ));
            }
            let total = list.len();
            let prev = ctx.vars.get(item_var).cloned();
            let prev_loop = ctx.vars.get("loop").cloned();
            for (idx, item) in list.into_iter().enumerate() {
                ctx.vars.insert(item_var.clone(), item);
                ctx.vars.insert(
                    "loop".to_string(),
                    json!({
                        "index": idx,
                        "index1": idx + 1,
                        "count": total,
                        "first": idx == 0,
                        "last": idx + 1 == total,
                    }),
                );
                match exec_steps(ctx, steps)? {
                    LoopSignal::None | LoopSignal::Continue => continue,
                    LoopSignal::Break => break,
                }
            }
            match prev {
                Some(v) => { ctx.vars.insert(item_var.clone(), v); }
                None => { ctx.vars.remove(item_var); }
            }
            match prev_loop {
                Some(v) => { ctx.vars.insert("loop".to_string(), v); }
                None => { ctx.vars.remove("loop"); }
            }
            Ok((Some(json!({ "status": "completed", "count": total })), LoopSignal::None))
        }
        ActionSpec::FlowWhile {
            condition,
            max_iterations,
            steps,
        } => {
            let cap = (*max_iterations).min(10_000);
            let mut iters = 0usize;
            let prev_loop = ctx.vars.get("loop").cloned();
            while ctx.eval_condition(condition) {
                if iters >= cap {
                    match prev_loop {
                        Some(v) => { ctx.vars.insert("loop".to_string(), v); }
                        None => { ctx.vars.remove("loop"); }
                    }
                    return Err(format!("`flow.while` a atteint la limite max_iterations={}", cap));
                }
                ctx.vars.insert(
                    "loop".to_string(),
                    json!({
                        "index": iters,
                        "index1": iters + 1,
                        "first": iters == 0,
                    }),
                );
                iters += 1;
                match exec_steps(ctx, steps)? {
                    LoopSignal::None | LoopSignal::Continue => continue,
                    LoopSignal::Break => break,
                }
            }
            match prev_loop {
                Some(v) => { ctx.vars.insert("loop".to_string(), v); }
                None => { ctx.vars.remove("loop"); }
            }
            Ok((Some(json!({ "iterations": iters })), LoopSignal::None))
        }
        _ => exec_leaf_action(ctx, step).map(|v| (v, LoopSignal::None)),
    }
}

fn exec_leaf_action(ctx: &mut RuntimeCtx<'_>, step: &StepSpec) -> Result<Option<Value>, String> {
    match &step.action {
        ActionSpec::FlowSet { params } => {
            let val = match &params.value {
                Value::String(s) => {
                    let rendered = ctx.render_str(s);
                    eval_arithmetic(&rendered).unwrap_or(Value::String(rendered))
                }
                other => other.clone(),
            };
            ctx.vars.insert(params.var.clone(), val.clone());
            Ok(Some(val))
        }
        ActionSpec::FlowAppend { params } => {
            let item = match &params.value {
                Value::String(s) => {
                    let rendered = ctx.render_str(s);
                    eval_arithmetic(&rendered).unwrap_or(Value::String(rendered))
                }
                other => other.clone(),
            };
            let mut arr = match ctx.vars.get(&params.var) {
                Some(Value::Array(a)) => a.clone(),
                None | Some(Value::Null) => Vec::new(),
                Some(other) => vec![other.clone()],
            };
            arr.push(item);
            let out = Value::Array(arr);
            ctx.vars.insert(params.var.clone(), out.clone());
            Ok(Some(out))
        }
        ActionSpec::FlowCall { params } => {
            let raw_path = ctx.render_str(&params.workflow_path);
            let yaml = fs::read_to_string(&raw_path)
                .map_err(|e| format!("flow.call '{}': {}", raw_path, e))?;
            let sub_spec: WorkflowSpec = serde_yaml_ng::from_str(&yaml)
                .map_err(|e| format!("flow.call parse '{}': {}", raw_path, e))?;
            let mut sub_inputs = HashMap::new();
            for (k, v) in &params.inputs {
                sub_inputs.insert(k.clone(), render_json_value(ctx, v));
            }
            let report = execute_workflow(
                &sub_spec,
                ExecutionOptions {
                    dry_run: ctx.dry_run,
                    inputs: sub_inputs,
                    secrets: ctx.secrets.clone(),
                    max_each_items: ctx.max_each_items,
                },
            )?;
            if !report.success {
                return Err(report.error.unwrap_or_else(|| format!("flow.call '{}' a échoué", raw_path)));
            }
            let last_output = report.logs.iter().rev().find_map(|l| l.output.clone());
            Ok(Some(json!({
                "workflow_name": report.workflow_name,
                "success": report.success,
                "last_output": last_output,
                "steps_count": report.logs.len(),
            })))
        }
        ActionSpec::DataParseJson { params } => {
            let rendered = ctx.render_str(&params.text);
            let parsed: Value = serde_json::from_str(&rendered)
                .map_err(|e| format!("data.parse_json: {}", e))?;
            Ok(Some(parsed))
        }
        ActionSpec::DataParseCsv { params } => {
            let rendered = ctx.render_str(&params.text);
            let delim = params.delimiter.as_str();
            let mut lines = rendered.lines().filter(|l| !l.trim().is_empty());
            let mut rows = Vec::new();
            if params.headers {
                if let Some(hdr_line) = lines.next() {
                    let headers: Vec<&str> = hdr_line.split(delim).map(str::trim).collect();
                    for line in lines {
                        let cols: Vec<&str> = line.split(delim).map(str::trim).collect();
                        let mut obj = serde_json::Map::new();
                        for (i, h) in headers.iter().enumerate() {
                            obj.insert((*h).to_string(), Value::String(cols.get(i).copied().unwrap_or("").to_string()));
                        }
                        rows.push(Value::Object(obj));
                    }
                }
            } else {
                for line in lines {
                    let cols: Vec<Value> = line.split(delim).map(|c| Value::String(c.trim().to_string())).collect();
                    rows.push(Value::Array(cols));
                }
            }
            Ok(Some(json!({ "rows": rows, "count": rows.len() })))
        }
        ActionSpec::DataRegex { params } => {
            let text = ctx.render_str(&params.text);
            let re = regex::Regex::new(&params.pattern)
                .map_err(|e| format!("data.regex pattern invalide: {}", e))?;
            let mut matches = Vec::new();
            for cap in re.captures_iter(&text) {
                let full = cap.get(0).map(|m| m.as_str()).unwrap_or("");
                let groups: Vec<Value> = cap
                    .iter()
                    .skip(1)
                    .map(|g| g.map(|m| Value::String(m.as_str().to_string())).unwrap_or(Value::Null))
                    .collect();
                matches.push(json!({ "match": full, "groups": groups }));
            }
            let first = matches.first().cloned().unwrap_or(Value::Null);
            Ok(Some(json!({ "matches": matches, "count": matches.len(), "first": first })))
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
        ActionSpec::VfsList { params } => {
            let dir = ctx.safe_local_path(&params.conn, &params.path)?;
            let glob = params.glob.as_deref().map(str::trim).filter(|g| !g.is_empty());
            let mut entries = Vec::new();
            collect_vfs_list(&dir, &dir, params.recursive, params.include_dirs, glob, &mut entries)?;
            let names: Vec<Value> = entries.iter().filter_map(|e| e.get("name").cloned()).collect();
            let stems: Vec<Value> = entries.iter().filter_map(|e| e.get("stem").cloned()).collect();
            let paths: Vec<Value> = entries.iter().filter_map(|e| e.get("path").cloned()).collect();
            let full_paths: Vec<Value> = entries.iter().filter_map(|e| e.get("full_path").cloned()).collect();
            let count = entries.len();
            Ok(Some(json!({
                "data": entries,
                "names": names,
                "stems": stems,
                "paths": paths,
                "full_paths": full_paths,
                "count": count
            })))
        }
        ActionSpec::VfsAppend { params } => {
            let path = ctx.safe_local_path(&params.conn, &params.path)?;
            let mut addition = ctx.render_str(&params.content);
            if params.newline && !addition.ends_with('\n') {
                addition.push('\n');
            }
            if ctx.dry_run {
                let mut cur = if ctx.dry_vfs_deleted.contains(&path) {
                    Vec::new()
                } else if let Some(mem) = ctx.dry_vfs_files.get(&path) {
                    mem.clone()
                } else {
                    fs::read(&path).unwrap_or_default()
                };
                cur.extend_from_slice(addition.as_bytes());
                let total = cur.len();
                ctx.dry_vfs_deleted.remove(&path);
                ctx.dry_vfs_files.insert(path, cur);
                return Ok(Some(json!({ "bytes": total, "dry_run": true })));
            }
            if params.create_dirs {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
            }
            use std::io::Write;
            let mut f = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| format!("vfs.append '{}': {}", path.display(), e))?;
            f.write_all(addition.as_bytes())
                .map_err(|e| format!("vfs.append '{}': {}", path.display(), e))?;
            Ok(Some(json!({ "appended_bytes": addition.len(), "dry_run": false })))
        }
        ActionSpec::VfsMkdir { params } => {
            let path = ctx.safe_local_path(&params.conn, &params.path)?;
            if !ctx.dry_run {
                fs::create_dir_all(&path).map_err(|e| format!("vfs.mkdir '{}': {}", path.display(), e))?;
            }
            Ok(Some(json!({ "created": true, "path": path.display().to_string(), "dry_run": ctx.dry_run })))
        }
        ActionSpec::VfsExists { params } => {
            let path = ctx.safe_local_path(&params.conn, &params.path)?;
            let exists = if ctx.dry_run {
                !ctx.dry_vfs_deleted.contains(&path)
                    && (ctx.dry_vfs_files.contains_key(&path) || path.exists())
            } else {
                path.exists()
            };
            Ok(Some(json!({ "exists": exists, "path": path.display().to_string() })))
        }
        ActionSpec::VfsMove { params } => {
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
                    return Err(format!("vfs.move: source supprimée en dry_run '{}'", src.display()));
                } else if let Some(mem) = ctx.dry_vfs_files.remove(&src) {
                    mem
                } else {
                    fs::read(&src).map_err(|e| format!("vfs.move '{}': {}", src.display(), e))?
                };
                ctx.dry_vfs_deleted.insert(src.clone());
                ctx.dry_vfs_deleted.remove(&dst);
                ctx.dry_vfs_files.insert(dst.clone(), bytes);
                return Ok(Some(json!({ "moved": true, "dry_run": true, "src": src.display().to_string(), "dst": dst.display().to_string() })));
            }
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            if fs::rename(&src, &dst).is_err() {
                fs::copy(&src, &dst).map_err(|e| format!("vfs.move copy: {}", e))?;
                fs::remove_file(&src).map_err(|e| format!("vfs.move remove src: {}", e))?;
            }
            Ok(Some(json!({ "moved": true, "src": src.display().to_string(), "dst": dst.display().to_string() })))
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
        ActionSpec::SysExec { params } => {
            let program = ctx.render_str(&params.program);
            let args: Vec<String> = params.args.iter().map(|a| ctx.render_str(a)).collect();
            let composed_cmd = if args.is_empty() {
                program.clone()
            } else {
                let quoted_args: Vec<String> = args
                    .iter()
                    .map(|a| {
                        if a.contains(char::is_whitespace) {
                            format!("\"{}\"", a.replace('"', "\\\""))
                        } else {
                            a.clone()
                        }
                    })
                    .collect();
                format!("{} {}", program, quoted_args.join(" "))
            };
            match shell_guard::inspect_command(&composed_cmd) {
                shell_guard::Verdict::DangerousBlocked { command: c, reason } => {
                    return Err(format!("sys.exec bloqué par shell-guard ('{}'): {}", c, reason));
                }
                shell_guard::Verdict::InteractiveBlocked {
                    command: c,
                    reason,
                    suggestion,
                } => {
                    return Err(format!(
                        "sys.exec commande interactive refusée ('{}'): {} Suggestion: {}",
                        c, reason, suggestion
                    ));
                }
                shell_guard::Verdict::Allowed { .. } => {}
            }
            if ctx.dry_run {
                return Ok(Some(json!({
                    "dry_run": true,
                    "program": program,
                    "args": args
                })));
            }
            let mut cmd = Command::new(&program);
            cmd.args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if let Some(ref cwd) = params.cwd {
                let rendered_cwd = ctx.render_str(cwd);
                let p = Path::new(&rendered_cwd);
                if tool_guard::path::is_system_sensitive(p) {
                    return Err(format!("cwd système refusé : '{}'", rendered_cwd));
                }
                cmd.current_dir(p);
            }
            for (k, v) in &params.env {
                cmd.env(k, ctx.render_str(v));
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
            }
            let mut child = cmd
                .spawn()
                .map_err(|e| format!("sys.exec '{}': {}", program, e))?;
            let mut child_out = child.stdout.take();
            let mut child_err = child.stderr.take();
            let out_handle = std::thread::spawn(move || {
                use std::io::Read;
                let mut buf = Vec::new();
                if let Some(ref mut r) = child_out {
                    let _ = r.read_to_end(&mut buf);
                }
                buf
            });
            let err_handle = std::thread::spawn(move || {
                use std::io::Read;
                let mut buf = Vec::new();
                if let Some(ref mut r) = child_err {
                    let _ = r.read_to_end(&mut buf);
                }
                buf
            });
            let exit_status = if let Some(ms) = step.timeout_ms {
                let deadline = std::time::Instant::now() + Duration::from_millis(ms.max(50));
                loop {
                    match child.try_wait() {
                        Ok(Some(st)) => break st,
                        Ok(None) => {
                            if std::time::Instant::now() >= deadline {
                                #[cfg(windows)]
                                {
                                    use std::os::windows::process::CommandExt;
                                    let pid = child.id().to_string();
                                    let _ = Command::new("taskkill")
                                        .args(["/T", "/F", "/PID", &pid])
                                        .creation_flags(0x0800_0000)
                                        .output();
                                }
                                let _ = child.kill();
                                let _ = child.wait();
                                let _ = out_handle.join();
                                let _ = err_handle.join();
                                return Err(format!("sys.exec '{}' timeout après {}ms", program, ms));
                            }
                            std::thread::sleep(Duration::from_millis(20));
                        }
                        Err(e) => return Err(format!("sys.exec '{}': {}", program, e)),
                    }
                }
            } else {
                child
                    .wait()
                    .map_err(|e| format!("sys.exec '{}': {}", program, e))?
            };
            let raw_out = out_handle.join().unwrap_or_default();
            let raw_err = err_handle.join().unwrap_or_default();
            let code = exit_status.code().unwrap_or(-1);
            let stdout = ctx.sanitize(&String::from_utf8_lossy(&raw_out));
            let stderr = ctx.sanitize(&String::from_utf8_lossy(&raw_err));
            if !exit_status.success() && !params.allow_non_zero {
                return Err(format!("sys.exec '{}' exit {}: {}", program, code, stderr.trim()));
            }
            Ok(Some(json!({
                "exit_code": code,
                "stdout": stdout.trim_end(),
                "stderr": stderr.trim_end()
            })))
        }
        ActionSpec::SysHttp { params } => {
            exec_sys_http(ctx, step.timeout_ms, params)
        }
        _ => Err("Action non implémentée dans le runtime local (Étape 2)".to_string()),
    }
}

fn render_json_value(ctx: &RuntimeCtx<'_>, val: &Value) -> Value {
    match val {
        Value::String(s) => Value::String(ctx.render_str(s)),
        Value::Array(arr) => Value::Array(arr.iter().map(|v| render_json_value(ctx, v)).collect()),
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), render_json_value(ctx, v));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn exec_sys_http(
    ctx: &RuntimeCtx<'_>,
    timeout_ms: Option<u64>,
    params: &crate::ast::SysHttpParams,
) -> Result<Option<Value>, String> {
    let mut headers: HashMap<String, String> = HashMap::new();
    let mut base_url: Option<String> = None;

    if let Some(ref conn_name) = params.conn {
        match ctx.spec.connections.get(conn_name) {
            Some(ConnectionSpec::Http {
                base_url: b,
                headers: h,
            }) => {
                base_url = Some(ctx.render_str(b));
                for (k, v) in h {
                    headers.insert(k.clone(), ctx.render_str(v));
                }
            }
            Some(_) => {
                return Err(format!(
                    "sys.http: connexion '{}' n'est pas de type `http`",
                    conn_name
                ))
            }
            None => return Err(format!("sys.http: connexion '{}' introuvable", conn_name)),
        }
    }

    for (k, v) in &params.headers {
        headers.insert(k.clone(), ctx.render_str(v));
    }

    let raw_url = ctx.render_str(&params.url);
    let full_url = if raw_url.starts_with("http://") || raw_url.starts_with("https://") {
        raw_url
    } else if let Some(b) = base_url {
        format!(
            "{}/{}",
            b.trim_end_matches('/'),
            raw_url.trim_start_matches('/')
        )
    } else {
        return Err(format!("sys.http: URL invalide sans base_url '{}'", raw_url));
    };

    if !(full_url.starts_with("http://") || full_url.starts_with("https://")) {
        return Err(format!("sys.http: schéma non supporté '{}'", full_url));
    }

    let method_str = params.method.trim().to_uppercase();
    let method = reqwest::Method::from_bytes(method_str.as_bytes())
        .map_err(|_| format!("sys.http: méthode HTTP invalide '{}'", params.method))?;

    let mut query: Vec<(String, String)> = Vec::with_capacity(params.query.len());
    for (k, v) in &params.query {
        query.push((k.clone(), ctx.render_str(v)));
    }

    if ctx.dry_run {
        return Ok(Some(json!({
            "dry_run": true,
            "method": method_str,
            "url": full_url,
            "query": query.into_iter().collect::<HashMap<String, String>>()
        })));
    }

    let timeout = Duration::from_millis(timeout_ms.unwrap_or(30_000).max(100));
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| format!("sys.http client error: {}", e))?;

    let mut req = client.request(method, &full_url);
    for (k, v) in &headers {
        req = req.header(k.as_str(), v.as_str());
    }
    if !query.is_empty() {
        req = req.query(&query);
    }
    if let Some(ref b) = params.body {
        let rendered = render_json_value(ctx, b);
        req = req.json(&rendered);
    }

    let resp = req.send().map_err(|e| format!("sys.http '{}': {}", full_url, e))?;
    let status = resp.status();
    let text = resp
        .text()
        .map_err(|e| format!("sys.http read body: {}", e))?;
    let sanitized = ctx.sanitize(&text);

    if !status.is_success() {
        return Err(format!(
            "sys.http {} {} -> HTTP {}: {}",
            method_str,
            full_url,
            status.as_u16(),
            sanitized
        ));
    }

    let body_val = if params.parse_json {
        serde_json::from_str::<Value>(&sanitized).unwrap_or_else(|_| Value::String(sanitized.clone()))
    } else {
        Value::String(sanitized)
    };

    Ok(Some(json!({
        "status": status.as_u16(),
        "body": body_val
    })))
}

fn unwrap_template_expr(raw: &str) -> &str {
    let t = raw.trim();
    t.strip_prefix("{{")
        .and_then(|s| s.strip_suffix("}}"))
        .or_else(|| t.strip_prefix("${").and_then(|s| s.strip_suffix('}')))
        .unwrap_or(t)
        .trim()
}

fn eval_arithmetic(raw: &str) -> Option<Value> {
    let s = raw.trim();
    for (idx, ch) in s.char_indices() {
        if idx == 0 || !matches!(ch, '+' | '-' | '*' | '/') {
            continue;
        }
        let lhs = s[..idx].trim();
        let rhs = s[idx + 1..].trim();
        if lhs.is_empty() || rhs.is_empty() {
            continue;
        }
        if let (Ok(a), Ok(b)) = (lhs.parse::<i64>(), rhs.parse::<i64>()) {
            return match ch {
                '+' => Some(Value::from(a.saturating_add(b))),
                '-' => Some(Value::from(a.saturating_sub(b))),
                '*' => Some(Value::from(a.saturating_mul(b))),
                '/' if b != 0 && a % b == 0 => Some(Value::from(a / b)),
                '/' if b != 0 => Some(json!((a as f64) / (b as f64))),
                _ => None,
            };
        }
        if let (Ok(a), Ok(b)) = (lhs.parse::<f64>(), rhs.parse::<f64>()) {
            let res = match ch {
                '+' => a + b,
                '-' => a - b,
                '*' => a * b,
                '/' if b != 0.0 => a / b,
                _ => return None,
            };
            return Some(json!(res));
        }
    }
    None
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_pi, mut match_ti) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_pi = Some(pi);
            match_ti = ti;
            pi += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            match_ti += 1;
            ti = match_ti;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn collect_vfs_list(
    base: &Path,
    dir: &Path,
    recursive: bool,
    include_dirs: bool,
    glob: Option<&str>,
    entries: &mut Vec<Value>,
) -> Result<(), String> {
    let rd = fs::read_dir(dir).map_err(|e| format!("vfs.list '{}': {}", dir.display(), e))?;
    for entry in rd.flatten() {
        let p = entry.path();
        let is_dir = p.is_dir();
        if is_dir && include_dirs {
            let name = entry.file_name().to_string_lossy().to_string();
            if glob.map_or(true, |pat| glob_match(pat, &name)) {
                let rel = p.strip_prefix(base).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                let parent_rel = Path::new(&rel).parent().map(|d| d.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                entries.push(json!({
                    "name": name,
                    "stem": name,
                    "ext": "",
                    "path": rel,
                    "full_path": p.to_string_lossy().replace('\\', "/"),
                    "dir": parent_rel,
                    "is_dir": true,
                    "size": 0,
                }));
            }
        }
        if is_dir {
            if recursive {
                collect_vfs_list(base, &p, true, include_dirs, glob, entries)?;
            }
            continue;
        }
        if p.is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(pat) = glob {
                if !glob_match(pat, &name) {
                    continue;
                }
            }
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| name.clone());
            let ext = p.extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let rel = p
                .strip_prefix(base)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            let parent_rel = Path::new(&rel).parent().map(|d| d.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            entries.push(json!({
                "name": name,
                "stem": stem,
                "ext": ext,
                "path": rel,
                "full_path": p.to_string_lossy().replace('\\', "/"),
                "dir": parent_rel,
                "is_dir": false,
                "size": size,
            }));
        }
    }
    Ok(())
}

fn value_len(val: &Value) -> usize {
    match val {
        Value::Array(a) => a.len(),
        Value::String(s) => s.len(),
        Value::Object(o) => o.len(),
        _ => 0,
    }
}

fn value_as_str(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn split_pipes_outside_quotes(expr: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut quote: Option<char> = None;
    for (i, ch) in expr.char_indices() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch == '|' => {
                parts.push(&expr[start..i]);
                start = i + 1;
            }
            None => {}
        }
    }
    parts.push(&expr[start..]);
    parts
}

fn parse_pipe_args(pipe: &str) -> (&str, Vec<String>) {
    if let Some((name, rest)) = pipe.split_once('(') {
        let inner = rest.strip_suffix(')').unwrap_or(rest);
        let mut args = Vec::new();
        let mut cur = String::new();
        let mut quote: Option<char> = None;
        for ch in inner.chars() {
            match quote {
                Some(q) if ch == q => quote = None,
                Some(_) => cur.push(ch),
                None if ch == '\'' || ch == '"' => quote = Some(ch),
                None if ch == ',' => {
                    args.push(cur.trim().to_string());
                    cur.clear();
                }
                None => cur.push(ch),
            }
        }
        if !cur.trim().is_empty() {
            args.push(cur.trim().to_string());
        }
        (name.trim(), args)
    } else {
        (pipe.trim(), Vec::new())
    }
}

fn apply_pipe(val: Value, pipe: &str) -> Option<Value> {
    let (name, args) = parse_pipe_args(pipe);
    let arg0 = args.first().map(String::as_str).unwrap_or("");
    let arg1 = args.get(1).map(String::as_str).unwrap_or("");
    match name {
        "length" | "len" | "count" => Some(Value::from(value_len(&val) as u64)),
        "upper" => Some(Value::String(value_as_str(&val).to_uppercase())),
        "lower" => Some(Value::String(value_as_str(&val).to_lowercase())),
        "trim" => Some(Value::String(value_as_str(&val).trim().to_string())),
        "stem" => {
            let s = value_as_str(&val);
            let stem = Path::new(&s).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or(s);
            Some(Value::String(stem))
        }
        "ext" => {
            let s = value_as_str(&val);
            let ext = Path::new(&s).extension().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
            Some(Value::String(ext))
        }
        "basename" => {
            let s = value_as_str(&val);
            let bn = Path::new(&s).file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or(s);
            Some(Value::String(bn))
        }
        "dirname" => {
            let s = value_as_str(&val);
            let dn = Path::new(&s).parent().map(|x| x.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            Some(Value::String(dn))
        }
        "replace" => Some(Value::String(value_as_str(&val).replace(arg0, arg1))),
        "split" => {
            let sep = if arg0.is_empty() { "," } else { arg0 };
            let items: Vec<Value> = value_as_str(&val).split(sep).map(|p| Value::String(p.to_string())).collect();
            Some(Value::Array(items))
        }
        "first" => match val {
            Value::Array(mut a) if !a.is_empty() => Some(a.remove(0)),
            Value::String(s) => s.chars().next().map(|c| Value::String(c.to_string())),
            _ => None,
        },
        "last" => match val {
            Value::Array(mut a) => a.pop(),
            Value::String(s) => s.chars().last().map(|c| Value::String(c.to_string())),
            _ => None,
        },
        "join" => match val {
            Value::Array(a) => {
                let sep = arg0.replace("\\n", "\n");
                let joined: Vec<String> = a.iter().map(value_as_str).collect();
                Some(Value::String(joined.join(&sep)))
            }
            other => Some(Value::String(value_as_str(&other))),
        },
        "map" => match val {
            Value::Array(a) => {
                let mapped: Vec<Value> = a
                    .into_iter()
                    .filter_map(|el| match el {
                        Value::Object(map) => map.get(arg0).cloned(),
                        other => apply_pipe(other, arg0),
                    })
                    .collect();
                Some(Value::Array(mapped))
            }
            _ => None,
        },
        "sort" => match val {
            Value::Array(mut a) => {
                a.sort_by(|x, y| value_as_str(x).cmp(&value_as_str(y)));
                Some(Value::Array(a))
            }
            other => Some(other),
        },
        "unique" => match val {
            Value::Array(a) => {
                let mut seen = HashSet::new();
                let mut out = Vec::new();
                for el in a {
                    let k = el.to_string();
                    if seen.insert(k) {
                        out.push(el);
                    }
                }
                Some(Value::Array(out))
            }
            other => Some(other),
        },
        _ => None,
    }
}

fn unwrap_outer_parens(mut s: &str) -> &str {
    loop {
        let t = s.trim();
        if t.starts_with('(') && t.ends_with(')') {
            let mut depth = 0i32;
            let mut quote: Option<char> = None;
            let mut wraps_all = true;
            for (i, ch) in t.char_indices() {
                match quote {
                    Some(q) if ch == q => quote = None,
                    Some(_) => {}
                    None if ch == '\'' || ch == '"' => quote = Some(ch),
                    None if ch == '(' => depth += 1,
                    None if ch == ')' => {
                        depth -= 1;
                        if depth == 0 && i < t.len() - 1 {
                            wraps_all = false;
                            break;
                        }
                    }
                    None => {}
                }
            }
            if wraps_all && depth == 0 {
                s = &t[1..t.len() - 1];
                continue;
            }
        }
        return t;
    }
}

fn split_logical_op<'a>(s: &'a str, op: &str) -> Option<(&'a str, &'a str)> {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let bytes = s.as_bytes();
    let op_bytes = op.as_bytes();
    let mut i = 0usize;
    while i + op_bytes.len() <= bytes.len() {
        let ch = bytes[i] as char;
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch == '(' => depth += 1,
            None if ch == ')' => depth -= 1,
            None if depth == 0 && &bytes[i..i + op_bytes.len()] == op_bytes => {
                return Some((s[..i].trim(), s[i + op_bytes.len()..].trim()));
            }
            None => {}
        }
        i += 1;
    }
    None
}

fn split_outside_quotes<'a>(s: &'a str, op: &str) -> Option<(&'a str, &'a str)> {
    let mut quote: Option<char> = None;
    let bytes = s.as_bytes();
    let op_bytes = op.as_bytes();
    let mut i = 0usize;
    while i + op_bytes.len() <= bytes.len() {
        let ch = bytes[i] as char;
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if &bytes[i..i + op_bytes.len()] == op_bytes => {
                return Some((&s[..i], &s[i + op_bytes.len()..]));
            }
            None => {}
        }
        i += 1;
    }
    None
}

fn eval_predicate(lv: &Value, rv: &Value, op: &str) -> bool {
    let ls = value_as_str(lv);
    let rs = value_as_str(rv);
    match op {
        "contains" => match lv {
            Value::Array(arr) => arr.iter().any(|el| el == rv || value_as_str(el) == rs),
            _ => ls.contains(&rs),
        },
        "starts_with" => ls.starts_with(&rs),
        "ends_with" => ls.ends_with(&rs),
        "matches" => regex::Regex::new(&rs).map(|re| re.is_match(&ls)).unwrap_or(false),
        "in" => match rv {
            Value::Array(arr) => arr.iter().any(|el| el == lv || value_as_str(el) == ls),
            _ => rs.contains(&ls),
        },
        _ => false,
    }
}
