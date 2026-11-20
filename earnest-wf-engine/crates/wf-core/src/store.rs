use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ast::WorkflowSpec;
use crate::parse_workflow_yaml;
use crate::runtime::{execute_workflow, ExecutionOptions, RunReport, StepLog};
use crate::validator::{validate_workflow, workflow_sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRecord {
    pub name: String,
    pub file_path: String,
    pub sha256: String,
    pub status: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunDetails {
    pub run_id: String,
    pub workflow_name: String,
    pub workflow_sha256: String,
    pub trigger_type: String,
    pub dry_run: bool,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error_summary: Option<String>,
    pub step_logs: Vec<StepLog>,
}

pub struct WfStore {
    conn: Connection,
}

fn now_epoch_str() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    secs.to_string()
}

fn gen_run_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("wfrun_{:x}", nanos)
}

impl WfStore {
    pub fn open(db_path: &Path) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let conn = Connection::open(db_path).map_err(|e| format!("SQLite open error: {}", e))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS workflows (
                 name TEXT PRIMARY KEY,
                 file_path TEXT NOT NULL,
                 sha256 TEXT NOT NULL,
                 status TEXT NOT NULL CHECK(status IN ('draft', 'sealed')),
                 updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS runs (
                 run_id TEXT PRIMARY KEY,
                 workflow_name TEXT NOT NULL,
                 workflow_sha256 TEXT NOT NULL,
                 trigger_type TEXT NOT NULL,
                 dry_run INTEGER NOT NULL DEFAULT 0,
                 status TEXT NOT NULL CHECK(status IN ('running', 'success', 'failed', 'cancelled')),
                 started_at TEXT NOT NULL,
                 finished_at TEXT,
                 error_summary TEXT
             );
             CREATE TABLE IF NOT EXISTS step_logs (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 run_id TEXT NOT NULL REFERENCES runs(run_id),
                 step_id TEXT NOT NULL,
                 attempt INTEGER NOT NULL DEFAULT 1,
                 status TEXT NOT NULL CHECK(status IN ('running', 'success', 'failed', 'skipped', 'dry_run')),
                 started_at TEXT NOT NULL,
                 duration_ms INTEGER,
                 output_json TEXT,
                 error_json TEXT
             );",
        )
        .map_err(|e| format!("SQLite init schema error: {}", e))?;
        Ok(Self { conn })
    }

    pub fn save_draft(&self, spec: &WorkflowSpec, file_path: &Path) -> Result<WorkflowRecord, String> {
        let report = validate_workflow(spec);
        if !report.valid {
            let msgs: Vec<String> = report.errors.iter().map(|e| e.to_string()).collect();
            return Err(format!("Workflow invalide :\n{}", msgs.join("\n")));
        }
        if let Some(parent) = file_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let yaml = serde_yaml_ng::to_string(spec).map_err(|e| e.to_string())?;
        fs::write(file_path, yaml).map_err(|e| format!("Écriture workflow impossible: {}", e))?;

        let now = now_epoch_str();
        let file_str = file_path.to_string_lossy().replace('\\', "/");
        self.conn
            .execute(
                "INSERT INTO workflows (name, file_path, sha256, status, updated_at)
                 VALUES (?1, ?2, ?3, 'draft', ?4)
                 ON CONFLICT(name) DO UPDATE SET
                   file_path = excluded.file_path,
                   sha256 = excluded.sha256,
                   status = 'draft',
                   updated_at = excluded.updated_at",
                params![spec.name, file_str, report.sha256, now],
            )
            .map_err(|e| e.to_string())?;

        Ok(WorkflowRecord {
            name: spec.name.clone(),
            file_path: file_str,
            sha256: report.sha256,
            status: "draft".to_string(),
            updated_at: now,
        })
    }

    pub fn seal_workflow(&self, name: &str) -> Result<WorkflowRecord, String> {
        let mut rec = self
            .get_workflow(name)?
            .ok_or_else(|| format!("Workflow '{}' introuvable", name))?;
        let yaml = fs::read_to_string(&rec.file_path)
            .map_err(|e| format!("Lecture '{}' impossible: {}", rec.file_path, e))?;
        let spec = parse_workflow_yaml(&yaml)?;
        let report = validate_workflow(&spec);
        if !report.valid {
            return Err("Impossible de sceller un workflow invalide".to_string());
        }
        let now = now_epoch_str();
        self.conn
            .execute(
                "UPDATE workflows SET sha256 = ?1, status = 'sealed', updated_at = ?2 WHERE name = ?3",
                params![report.sha256, now, name],
            )
            .map_err(|e| e.to_string())?;
        rec.sha256 = report.sha256;
        rec.status = "sealed".to_string();
        rec.updated_at = now;
        Ok(rec)
    }

    pub fn get_workflow(&self, name: &str) -> Result<Option<WorkflowRecord>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT name, file_path, sha256, status, updated_at FROM workflows WHERE name = ?1")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query(params![name]).map_err(|e| e.to_string())?;
        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            Ok(Some(WorkflowRecord {
                name: row.get(0).map_err(|e| e.to_string())?,
                file_path: row.get(1).map_err(|e| e.to_string())?,
                sha256: row.get(2).map_err(|e| e.to_string())?,
                status: row.get(3).map_err(|e| e.to_string())?,
                updated_at: row.get(4).map_err(|e| e.to_string())?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_workflows(&self) -> Result<Vec<WorkflowRecord>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT name, file_path, sha256, status, updated_at FROM workflows ORDER BY name ASC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(WorkflowRecord {
                    name: row.get(0)?,
                    file_path: row.get(1)?,
                    sha256: row.get(2)?,
                    status: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    pub fn execute_and_record(
        &self,
        name: &str,
        opts: ExecutionOptions,
    ) -> Result<(String, RunReport), String> {
        let rec = self
            .get_workflow(name)?
            .ok_or_else(|| format!("Workflow '{}' introuvable", name))?;
        let yaml = fs::read_to_string(&rec.file_path)
            .map_err(|e| format!("Lecture '{}' impossible: {}", rec.file_path, e))?;
        let spec = parse_workflow_yaml(&yaml)?;
        let disk_sha = workflow_sha256(&spec);

        if !opts.dry_run {
            if rec.status != "sealed" {
                return Err(format!(
                    "Workflow '{}' est en statut '{}' : exécution réelle refusée (utilisez dry_run=true ou scellez-le d'abord).",
                    name, rec.status
                ));
            }
            if disk_sha != rec.sha256 {
                return Err(format!(
                    "Sceau SHA-256 invalide pour '{}' (disque {} != verrou {}) : exécution réelle bloquée.",
                    name, disk_sha, rec.sha256
                ));
            }
        }

        let run_id = gen_run_id();
        let started_at = now_epoch_str();
        self.conn
            .execute(
                "INSERT INTO runs (run_id, workflow_name, workflow_sha256, trigger_type, dry_run, status, started_at)
                 VALUES (?1, ?2, ?3, 'manual', ?4, 'running', ?5)",
                params![run_id, name, disk_sha, i32::from(opts.dry_run), started_at],
            )
            .map_err(|e| e.to_string())?;

        let report = execute_workflow(&spec, opts)?;
        let finished_at = now_epoch_str();
        let status = if report.success { "success" } else { "failed" };

        self.conn
            .execute(
                "UPDATE runs SET status = ?1, finished_at = ?2, error_summary = ?3 WHERE run_id = ?4",
                params![status, finished_at, report.error, run_id],
            )
            .map_err(|e| e.to_string())?;

        for log in &report.logs {
            let out_json = log.output.as_ref().map(|v| v.to_string());
            self.conn
                .execute(
                    "INSERT INTO step_logs (run_id, step_id, attempt, status, started_at, duration_ms, output_json, error_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7)",
                    params![
                        run_id,
                        log.step_id,
                        log.attempt,
                        log.status,
                        finished_at,
                        out_json,
                        log.error
                    ],
                )
                .map_err(|e| e.to_string())?;
        }

        Ok((run_id, report))
    }

    pub fn inspect_run(&self, run_id: &str) -> Result<Option<RunDetails>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT run_id, workflow_name, workflow_sha256, trigger_type, dry_run, status, started_at, finished_at, error_summary
                 FROM runs WHERE run_id = ?1",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query(params![run_id]).map_err(|e| e.to_string())?;
        let Some(row) = rows.next().map_err(|e| e.to_string())? else {
            return Ok(None);
        };

        let mut log_stmt = self
            .conn
            .prepare(
                "SELECT step_id, status, attempt, output_json, error_json
                 FROM step_logs WHERE run_id = ?1 ORDER BY id ASC",
            )
            .map_err(|e| e.to_string())?;
        let log_rows = log_stmt
            .query_map(params![run_id], |r| {
                let out_str: Option<String> = r.get(3)?;
                let output: Option<Value> = out_str.and_then(|s| serde_json::from_str(&s).ok());
                Ok(StepLog {
                    step_id: r.get(0)?,
                    status: r.get(1)?,
                    attempt: r.get(2)?,
                    output,
                    error: r.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut step_logs = Vec::new();
        for lr in log_rows {
            step_logs.push(lr.map_err(|e| e.to_string())?);
        }

        Ok(Some(RunDetails {
            run_id: row.get(0).map_err(|e| e.to_string())?,
            workflow_name: row.get(1).map_err(|e| e.to_string())?,
            workflow_sha256: row.get(2).map_err(|e| e.to_string())?,
            trigger_type: row.get(3).map_err(|e| e.to_string())?,
            dry_run: row.get::<_, i32>(4).map_err(|e| e.to_string())? != 0,
            status: row.get(5).map_err(|e| e.to_string())?,
            started_at: row.get(6).map_err(|e| e.to_string())?,
            finished_at: row.get(7).map_err(|e| e.to_string())?,
            error_summary: row.get(8).map_err(|e| e.to_string())?,
            step_logs,
        }))
    }
}
