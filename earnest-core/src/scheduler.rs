//! Planificateur périodique UTC autonome propulsé par `cron_pulse`.
//!
//! Déclenche une seule fois par minute UTC :
//! 1. Les missions agentiques actives de `scheduled_tasks` (`is_active = 1`) via `ReActEngine`.
//! 2. Les workflows WFL scellés et activés (`status = 'sealed' AND enabled = 1`) possédant un `cron_expr`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cron_pulse::{civil_from_unix, CronSchedule};
use rusqlite::{params, Connection};

use crate::db::connection::get_connection;
use crate::engine::react::{push_live_log, ReActEngine};
use crate::llm::router::LLMRouter;

static SCHEDULER_STARTED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone)]
pub struct DueScheduledTask {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub cron_expr: String,
    pub project_id: String,
}

/// Formate un timestamp Unix en clé de minute UTC canonique (`YYYY-MM-DD HH:MM UTC`).
pub fn format_utc_minute(unix_secs: u64) -> String {
    let dt = civil_from_unix(unix_secs);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        dt.year, dt.month, dt.day, dt.hour, dt.minute
    )
}

/// Inspecte `scheduled_tasks`, marque immédiatement `last_run_at` sur les tâches dues
/// pour garantir l'idempotence à la minute UTC près, et retourne la liste à exécuter.
pub fn claim_due_scheduled_tasks(conn: &Connection, now_unix_secs: u64) -> Vec<DueScheduledTask> {
    let minute_stamp = format_utc_minute(now_unix_secs);
    let mut candidates = Vec::new();

    if let Ok(mut stmt) = conn.prepare(
        "SELECT id, name, prompt, cron_expr, project_id, last_run_at
         FROM scheduled_tasks
         WHERE is_active = 1",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        }) {
            for r in rows.flatten() {
                candidates.push(r);
            }
        }
    }

    let mut due = Vec::new();
    for (id, name, prompt, cron_expr, project_id, last_run_at) in candidates {
        if last_run_at.as_deref() == Some(minute_stamp.as_str()) {
            continue;
        }
        let Ok(sched) = CronSchedule::parse(&cron_expr) else {
            continue;
        };
        if !sched.matches(now_unix_secs) {
            continue;
        }
        if conn
            .execute(
                "UPDATE scheduled_tasks SET last_run_at = ?1 WHERE id = ?2 AND (last_run_at IS NULL OR last_run_at != ?1)",
                params![minute_stamp, id],
            )
            .unwrap_or(0)
            == 1
        {
            due.push(DueScheduledTask {
                id,
                name,
                prompt,
                cron_expr,
                project_id,
            });
        }
    }

    due
}

/// Démarre le démon planificateur UTC en tâche de fond (idempotent : un seul thread par processus).
pub fn start_background_scheduler() {
    if SCHEDULER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    thread::spawn(|| {
        loop {
            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            if now_secs > 0 {
                // 1. Missions agentiques `scheduled_tasks`
                if let Ok(conn) = get_connection(None) {
                    let due_tasks = claim_due_scheduled_tasks(&conn, now_secs);
                    for task in due_tasks {
                        push_live_log(
                            "INFO",
                            &format!(
                                "[CRON] Déclenchement automatique de la tâche '{}' ({})",
                                task.name, task.cron_expr
                            ),
                        );
                        thread::spawn(move || {
                            if let Ok(run_conn) = get_connection(None) {
                                let router = LLMRouter::from_db(&run_conn);
                                let engine = ReActEngine::new(&run_conn, &router, &task.project_id);
                                if let Ok(res) =
                                    engine.run_with_callback(&task.prompt, |_, _, _, _, _, _| {})
                                {
                                    let _ = run_conn.execute(
                                        "UPDATE runs SET task_id = ?1 WHERE id = ?2",
                                        params![task.id, res.run_id],
                                    );
                                }
                            }
                        });
                    }
                }

                // 2. Workflows WFL scellés & activés ayant une expression cron
                let ws = crate::server::discover_wf_workspace();
                let wf_db = ws.join(".wf").join("wf.db");
                if wf_db.exists() {
                    if let Ok(store) = wf_core::WfStore::open(&wf_db) {
                        if let Ok(fired) = store.run_due_cron_workflows(now_secs) {
                            for (wf_name, run_id) in fired {
                                push_live_log(
                                    "INFO",
                                    &format!(
                                        "[CRON-WFL] Workflow '{}' exécuté automatiquement ({})",
                                        wf_name, run_id
                                    ),
                                );
                            }
                        }
                    }
                }
            }

            thread::sleep(Duration::from_secs(20));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connection::init_db;

    #[test]
    fn claim_due_scheduled_tasks_fires_once_per_utc_minute() {
        let tmp = std::env::temp_dir().join(format!("earnest_cron_test_{}.db", uuid::Uuid::new_v4()));
        init_db(Some(&tmp)).expect("init temp db");
        let conn = get_connection(Some(&tmp)).expect("open temp db");

        conn.execute(
            "INSERT INTO scheduled_tasks (id, name, prompt, cron_expr, project_id, is_active)
             VALUES ('t1', 'daily_9am', 'Summarize logs', '0 9 * * MON-FRI', 'default_workspace', 1)",
            [],
        )
        .unwrap();

        // 2024-03-15T09:00:12Z (Friday)
        let matching_ts = cron_pulse::unix_from_civil(2024, 3, 15, 9, 0, 12).unwrap();
        let first = claim_due_scheduled_tasks(&conn, matching_ts);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].id, "t1");

        // Second check 25s later inside the same UTC minute -> must NOT re-trigger
        let second = claim_due_scheduled_tasks(&conn, matching_ts + 25);
        assert!(second.is_empty());

        // Non-matching Saturday 09:00 -> must NOT trigger
        let saturday_ts = cron_pulse::unix_from_civil(2024, 3, 16, 9, 0, 0).unwrap();
        assert!(claim_due_scheduled_tasks(&conn, saturday_ts).is_empty());

        // Next Monday 09:00 -> triggers again
        let monday_ts = cron_pulse::unix_from_civil(2024, 3, 18, 9, 0, 5).unwrap();
        let third = claim_due_scheduled_tasks(&conn, monday_ts);
        assert_eq!(third.len(), 1);

        let _ = std::fs::remove_file(&tmp);
    }
}
