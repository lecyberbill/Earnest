use std::path::{Path, PathBuf};
use std::fs;
use rusqlite::{Connection, Result};
use super::schema::SCHEMA_SQL;

pub fn get_data_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let dir = home.join(".local").join("share").join("agent-manager");
    fs::create_dir_all(&dir).ok();
    dir
}

pub fn get_db_path() -> PathBuf {
    get_data_dir().join("data.db")
}

pub fn migrate_db(conn: &Connection) {
    let runs_sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'runs'",
            [],
            |r| r.get(0),
        )
        .ok();
    if let Some(sql) = runs_sql {
        if !sql.contains("final_answer") || !sql.contains("awaiting_approval") {
            let has_final_col = sql.contains("final_answer");
            let select_final = if has_final_col { "final_answer" } else { "NULL" };
            let mig = format!(
                "PRAGMA foreign_keys = OFF;
                 BEGIN;
                 CREATE TABLE runs_v2 (
                     id TEXT PRIMARY KEY,
                     project_id TEXT NOT NULL,
                     task_id TEXT REFERENCES scheduled_tasks(id) ON DELETE SET NULL,
                     prompt TEXT NOT NULL,
                     status TEXT CHECK(status IN ('running', 'success', 'failed', 'cancelled', 'awaiting_approval')),
                     provider TEXT NOT NULL,
                     model TEXT NOT NULL,
                     prompt_tokens INTEGER DEFAULT 0,
                     completion_tokens INTEGER DEFAULT 0,
                     total_tokens INTEGER DEFAULT 0,
                     duration_ms INTEGER DEFAULT 0,
                     final_answer TEXT,
                     created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                     completed_at DATETIME
                 );
                 INSERT INTO runs_v2 (id, project_id, task_id, prompt, status, provider, model, prompt_tokens, completion_tokens, total_tokens, duration_ms, final_answer, created_at, completed_at)
                 SELECT id, project_id, task_id, prompt, status, provider, model, prompt_tokens, completion_tokens, total_tokens, duration_ms, {}, created_at, completed_at FROM runs;
                 DROP TABLE runs;
                 ALTER TABLE runs_v2 RENAME TO runs;
                 UPDATE runs
                 SET status = 'success',
                     completed_at = COALESCE(completed_at, CURRENT_TIMESTAMP),
                     final_answer = (
                         SELECT payload FROM run_traces rt
                         WHERE rt.run_id = runs.id AND rt.step_type = 'answer'
                         ORDER BY rt.step_order DESC LIMIT 1
                     )
                 WHERE status = 'running' AND EXISTS (
                     SELECT 1 FROM run_traces rt WHERE rt.run_id = runs.id AND rt.step_type = 'answer'
                 );
                 COMMIT;
                 PRAGMA foreign_keys = ON;",
                select_final
            );
            let _ = conn.execute_batch(&mig);
        }
    }

    let traces_sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'run_traces'",
            [],
            |r| r.get(0),
        )
        .ok();
    if let Some(tsql) = traces_sql {
        if !tsql.contains("wfl_suggestion") {
            let _ = conn.execute_batch(
                "PRAGMA foreign_keys = OFF;
                 BEGIN;
                 CREATE TABLE run_traces_v2 (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
                     step_order INTEGER NOT NULL,
                     step_type TEXT CHECK(step_type IN ('thought', 'tool_call', 'tool_output', 'error', 'answer', 'wfl_suggestion')),
                     tool_name TEXT,
                     payload TEXT,
                     created_at DATETIME DEFAULT CURRENT_TIMESTAMP
                 );
                 INSERT INTO run_traces_v2 (id, run_id, step_order, step_type, tool_name, payload, created_at)
                 SELECT id, run_id, step_order, step_type, tool_name, payload, created_at FROM run_traces;
                 DROP TABLE run_traces;
                 ALTER TABLE run_traces_v2 RENAME TO run_traces;
                 COMMIT;
                 PRAGMA foreign_keys = ON;",
            );
        }
    }
}

pub fn get_connection(path_opt: Option<&Path>) -> Result<Connection> {
    let db_path = match path_opt {
        Some(p) => p.to_path_buf(),
        None => get_db_path(),
    };

    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).ok();
    }

    let conn = Connection::open(&db_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;",
    )?;
    conn.execute_batch(SCHEMA_SQL)?;
    migrate_db(&conn);
    Ok(conn)
}

pub fn init_db(path_opt: Option<&Path>) -> Result<Connection> {
    get_connection(path_opt)
}

#[cfg(test)]
mod tests {
    use super::migrate_db;
    use rusqlite::Connection;

    #[test]
    fn migrate_db_upgrades_legacy_runs_and_backfills_answers() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE scheduled_tasks (id TEXT PRIMARY KEY);
            CREATE TABLE runs (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                task_id TEXT,
                prompt TEXT NOT NULL,
                status TEXT CHECK(status IN ('running', 'success', 'failed', 'cancelled')),
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                prompt_tokens INTEGER DEFAULT 0,
                completion_tokens INTEGER DEFAULT 0,
                total_tokens INTEGER DEFAULT 0,
                duration_ms INTEGER DEFAULT 0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                completed_at DATETIME
            );
            CREATE TABLE run_traces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
                step_order INTEGER NOT NULL,
                step_type TEXT CHECK(step_type IN ('thought', 'tool_call', 'tool_output', 'error', 'answer')),
                tool_name TEXT,
                payload TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            INSERT INTO runs (id, project_id, prompt, status, provider, model) VALUES ('r1', 'p1', 'hello', 'running', 'openai', 'ds');
            INSERT INTO run_traces (run_id, step_order, step_type, payload) VALUES ('r1', 1, 'answer', 'done!');",
        )
        .unwrap();

        migrate_db(&conn);

        let (status, ans): (String, Option<String>) = conn
            .query_row("SELECT status, final_answer FROM runs WHERE id = 'r1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(status, "success");
        assert_eq!(ans.as_deref(), Some("done!"));

        conn.execute(
            "INSERT INTO runs (id, project_id, prompt, status, provider, model) VALUES ('r2', 'p1', 'spec', 'awaiting_approval', 'openai', 'ds')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO run_traces (run_id, step_order, step_type, payload) VALUES ('r2', 1, 'wfl_suggestion', 'hint')",
            [],
        )
        .unwrap();
    }
}

