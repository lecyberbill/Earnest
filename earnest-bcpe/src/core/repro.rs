//! Rejeu des cas de test reproductibles (`kb revalidate`).
//!
//! C'est le seul module du binaire qui lance un processus externe. Garde-fous :
//! - jamais appelé par le serveur MCP (uniquement par la ligne de commande humaine) ;
//! - seuls les runners déclarés dans `config/runners.yaml` peuvent s'exécuter, sans shell ;
//! - un cas de test n'est exécuté qu'après approbation humaine (`kb approve-repro`) de la
//!   version de contenu courante de l'entrée ;
//! - délai maximal par runner ; tout échec de lancement est remonté, jamais remplacé.

use std::fs::{self, File};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rusqlite::{params, params_from_iter, Connection};

use crate::config::{AppConfig, Runner};
use crate::core::redact::redact_text;
use crate::core::time::now_iso;
use crate::error::{KbError, KbResult};
use crate::models::ReplayReport;

struct Candidate {
    entry_id: i64,
    subject_key: String,
    version: i64,
    runner: String,
    script: String,
    expected_exit: i64,
    expected_contains: Option<String>,
    approved_by: Option<String>,
    approved_version: Option<i64>,
}

struct Execution {
    exit_code: i64,
    output: String,
    command_line: String,
}

fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn execute(runner: &Runner, entry_id: i64, script: &str) -> Result<Execution, String> {
    let dir = std::env::temp_dir().join(format!("kb-replay-{}-{entry_id}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|e| format!("répertoire temporaire: {e}"))?;
    let result = execute_in(runner, &dir, script);
    let _ = fs::remove_dir_all(&dir);
    result
}

fn execute_in(runner: &Runner, dir: &Path, script: &str) -> Result<Execution, String> {
    let script_path = dir.join(format!("script.{}", runner.extension));
    fs::write(&script_path, script).map_err(|e| format!("écriture du script de rejeu: {e}"))?;
    let stdout_path = dir.join("stdout.txt");
    let stderr_path = dir.join("stderr.txt");
    let stdout = File::create(&stdout_path).map_err(|e| format!("fichier de sortie: {e}"))?;
    let stderr = File::create(&stderr_path).map_err(|e| format!("fichier d'erreur: {e}"))?;
    let script_text = script_path.to_string_lossy().to_string();
    let args: Vec<String> = runner
        .args
        .iter()
        .map(|arg| arg.replace("{file}", &script_text))
        .collect();
    let command_line = format!("{} {}", runner.program, args.join(" "));

    let mut child = Command::new(&runner.program)
        .args(&args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|e| format!("lancement impossible de '{}': {e}", runner.program))?;

    let deadline = Instant::now() + Duration::from_secs(runner.timeout_s);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "délai de {} s dépassé pour '{}'",
                        runner.timeout_s, runner.program
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("attente du processus '{}': {e}", runner.program)),
        }
    };
    let exit_code = status.code().map(i64::from).ok_or_else(|| {
        format!(
            "le processus '{}' s'est terminé sans code de sortie (signal)",
            runner.program
        )
    })?;
    let read = |path: &Path| -> String {
        fs::read(path)
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            .unwrap_or_default()
    };
    let output = format!("{}{}", read(&stdout_path), read(&stderr_path));
    Ok(Execution {
        exit_code,
        output,
        command_line,
    })
}

fn candidates(
    conn: &Connection,
    product: Option<&str>,
    entry: Option<i64>,
) -> KbResult<Vec<Candidate>> {
    let mut sql = String::from(
        "SELECT e.entry_id, e.subject_key, e.version, r.runner, r.script, r.expected_exit,
                r.expected_contains, r.approved_by, r.approved_version
         FROM entries e JOIN entry_repro r ON r.entry_id = e.entry_id
         WHERE e.status = 'a_revoir'",
    );
    let mut values: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(product) = product {
        sql.push_str(" AND e.product_id = ?");
        values.push(rusqlite::types::Value::Text(product.to_string()));
    }
    if let Some(entry) = entry {
        sql.push_str(" AND e.entry_id = ?");
        values.push(rusqlite::types::Value::Integer(entry));
    }
    sql.push_str(" ORDER BY e.entry_id");
    let mut stmt = conn.prepare(&sql).map_err(KbError::Db)?;
    let rows = stmt
        .query_map(params_from_iter(values.iter()), |row| {
            Ok(Candidate {
                entry_id: row.get(0)?,
                subject_key: row.get(1)?,
                version: row.get(2)?,
                runner: row.get(3)?,
                script: row.get(4)?,
                expected_exit: row.get(5)?,
                expected_contains: row.get(6)?,
                approved_by: row.get(7)?,
                approved_version: row.get(8)?,
            })
        })
        .map_err(KbError::Db)?;
    let mut output = Vec::new();
    for row in rows {
        output.push(row.map_err(KbError::Db)?);
    }
    Ok(output)
}

fn record_pass(
    conn: &mut Connection,
    config: &AppConfig,
    candidate: &Candidate,
    execution: &Execution,
) -> KbResult<()> {
    let actor = format!("rejeu:{}", candidate.runner);
    let timestamp = now_iso();
    let excerpt = {
        let redacted = redact_text(config, &truncate(execution.output.trim(), 2000));
        if redacted.trim().is_empty() {
            "(sortie vide)".to_string()
        } else {
            redacted
        }
    };
    let command = redact_text(config, &truncate(&execution.command_line, 1000));
    let tx = conn.transaction().map_err(KbError::Db)?;
    tx.execute(
        "INSERT INTO entry_outcomes (entry_id, outcome, note, actor, created_at)
         VALUES (?1, 'ok', 'rejeu approuvé réussi', ?2, ?3)",
        params![candidate.entry_id, actor, timestamp],
    )
    .map_err(KbError::Db)?;
    tx.execute(
        "INSERT INTO entry_evidence
         (entry_id, entry_version, actor, command, exit_code, output_excerpt, note, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'rejeu approuvé', ?7)",
        params![
            candidate.entry_id,
            candidate.version,
            actor,
            command,
            execution.exit_code,
            excerpt,
            timestamp
        ],
    )
    .map_err(KbError::Db)?;
    tx.execute(
        "UPDATE entries SET status='valide', validation_mode='auto', validated_by=?1,
                validated_at=?2, updated_at=?2, use_count = use_count + 1,
                product_version = COALESCE(
                    (SELECT current_version FROM products WHERE product_id = entries.product_id),
                    product_version)
         WHERE entry_id=?3",
        params![actor, timestamp, candidate.entry_id],
    )
    .map_err(KbError::Db)?;
    tx.commit().map_err(KbError::Db)
}

fn record_failure(conn: &mut Connection, candidate: &Candidate, note: &str) -> KbResult<()> {
    let actor = format!("rejeu:{}", candidate.runner);
    let tx = conn.transaction().map_err(KbError::Db)?;
    tx.execute(
        "INSERT INTO entry_outcomes (entry_id, outcome, note, actor, created_at)
         VALUES (?1, 'ko', ?2, ?3, ?4)",
        params![candidate.entry_id, note, actor, now_iso()],
    )
    .map_err(KbError::Db)?;
    tx.execute(
        "UPDATE entries SET fail_count = fail_count + 1, validation_mode = NULL,
                evidence_floor = (SELECT COALESCE(MAX(evidence_id), 0) FROM entry_evidence)
         WHERE entry_id = ?1",
        params![candidate.entry_id],
    )
    .map_err(KbError::Db)?;
    tx.commit().map_err(KbError::Db)
}

/// Rejoue les cas de test approuvés des entrées `a_revoir`.
/// Un rejeu réussi remet l'entrée en `valide` (validation_mode=auto) et aligne sa version produit ;
/// un rejeu en échec enregistre un `ko` ; une erreur d'exécution est remontée sans changer l'entrée.
pub fn revalidate(
    conn: &mut Connection,
    config: &AppConfig,
    product: Option<&str>,
    entry: Option<i64>,
) -> KbResult<Vec<ReplayReport>> {
    let mut reports = Vec::new();
    for candidate in candidates(conn, product, entry)? {
        let report = |outcome: &'static str, detail: String| ReplayReport {
            entry_id: candidate.entry_id,
            subject_key: candidate.subject_key.clone(),
            outcome,
            detail,
        };
        if candidate.approved_by.is_none() {
            reports.push(report(
                "non_approuve",
                "cas de test non approuvé: kb approve-repro <entry_id> --by humain:nom".to_string(),
            ));
            continue;
        }
        if candidate.approved_version != Some(candidate.version) {
            reports.push(report(
                "approbation_perimee",
                format!(
                    "l'entrée est en version {} mais l'approbation porte sur une autre version: \
                     relisez puis kb approve-repro",
                    candidate.version
                ),
            ));
            continue;
        }
        let runner = match config.runners.get(&candidate.runner) {
            Some(runner) => runner,
            None => {
                reports.push(report(
                    "erreur",
                    format!(
                        "runner '{}' non déclaré dans config/runners.yaml",
                        candidate.runner
                    ),
                ));
                continue;
            }
        };
        let execution = match execute(runner, candidate.entry_id, &candidate.script) {
            Ok(execution) => execution,
            Err(message) => {
                reports.push(report("erreur", message));
                continue;
            }
        };
        let exit_ok = execution.exit_code == candidate.expected_exit;
        let contains_ok = candidate
            .expected_contains
            .as_deref()
            .map(|needle| execution.output.contains(needle))
            .unwrap_or(true);
        if exit_ok && contains_ok {
            record_pass(conn, config, &candidate, &execution)?;
            reports.push(report(
                "ok",
                format!("code de sortie {} conforme", execution.exit_code),
            ));
        } else {
            let note = redact_text(
                config,
                &format!(
                    "rejeu en échec: code {} (attendu {}), texte attendu {}",
                    execution.exit_code,
                    candidate.expected_exit,
                    if contains_ok { "présent" } else { "absent" }
                ),
            );
            record_failure(conn, &candidate, &note)?;
            reports.push(report("echec", note));
        }
    }
    Ok(reports)
}
