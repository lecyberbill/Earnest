// Sondes d'invariants P01..P17 (voir tests/INVARIANTS.md).
// Les sondes pilotent le serveur MCP (JSON) et le binaire `kb` en boîte noire.
// Les tests lancent le binaire `kb` du projet et, pour P17, PowerShell : ce sont des
// processus de test déclarés, pas des chemins de production.
mod common;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use common::{error_code, mcp_call, search_input, TestKb};
use kb::config::AppConfig;
use kb::mcp::{tool_definitions, McpServer};

const KB_EXE: &str = env!("CARGO_BIN_EXE_kb");

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn server_as(kb: &TestKb, actor: &str) -> McpServer {
    McpServer::new(kb.open_second(), kb.config.clone()).with_actor(actor)
}

fn propose_as(server: &mut McpServer, subject: &str) -> i64 {
    let value = mcp_call(
        server,
        "kb_propose",
        json!({
            "product_id": "produit-test",
            "subject_key": subject,
            "kind": "piege",
            "title": "titre",
            "correction": "correction",
            "source": "agent",
            "terms": ["zorglub"]
        }),
    );
    value["entry_id"]
        .as_i64()
        .unwrap_or_else(|| panic!("entry_id manquant: {value}"))
}

fn confirm_as(server: &mut McpServer, entry_id: i64) -> Value {
    mcp_call(
        server,
        "kb_confirm",
        json!({
            "entry_id": entry_id,
            "command": "produit run test.script",
            "exit_code": 0,
            "output_excerpt": "12/12 OK"
        }),
    )
}

fn state_of(kb: &TestKb, entry_id: i64) -> (String, Option<String>) {
    kb.conn
        .query_row(
            "SELECT status, validation_mode FROM entries WHERE entry_id=?1",
            [entry_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("état de l'entrée")
}

fn count(kb: &TestKb, sql: &str) -> i64 {
    kb.conn.query_row(sql, [], |row| row.get(0)).expect("count")
}

fn cli(kb: &TestKb, config_dir: &Path, args: &[&str]) -> (i32, String) {
    let output = Command::new(KB_EXE)
        .arg("--db")
        .arg(&kb.db_path)
        .arg("--config")
        .arg(config_dir)
        .args(args)
        .output()
        .expect("lancement de kb");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code().unwrap_or(-1), text)
}

fn temp_config(extra: &[(&str, String)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for name in ["extractors.yaml", "redaction.yaml"] {
        fs::copy(manifest_dir().join("config").join(name), dir.path().join(name)).expect("copie");
    }
    for (name, content) in extra {
        fs::write(dir.path().join(name), content).expect("écriture config");
    }
    dir
}

fn runners_yaml(runners: &[(&str, &str, &[&str])]) -> String {
    let mut out = String::from("runners:\n");
    for (name, program, args) in runners {
        out.push_str(&format!("  {name}:\n    program: '{program}'\n    args: ["));
        out.push_str(
            &args
                .iter()
                .map(|a| format!("'{a}'"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str("]\n    timeout_s: 30\n");
    }
    out
}

#[test]
fn p01_auto_validate_field_rejected() {
    let kb = TestKb::new();
    let mut server = server_as(&kb, "agent:a");
    let response = mcp_call(
        &mut server,
        "kb_propose",
        json!({
            "product_id": "produit-test", "subject_key": "auto-champ", "kind": "piege",
            "title": "t", "correction": "c", "source": "agent", "auto_validate": true
        }),
    );
    assert_eq!(error_code(&response), Some("invalid_argument"), "{response}");
    assert_eq!(
        count(&kb, "SELECT COUNT(*) FROM entries WHERE subject_key='auto-champ'"),
        0
    );
}

#[test]
fn p02_proposer_cannot_self_promote() {
    let kb = TestKb::new();
    let mut author = server_as(&kb, "agent:a");
    let id = propose_as(&mut author, "auto-promotion");
    for _ in 0..5 {
        let response = confirm_as(&mut author, id);
        assert!(error_code(&response).is_none(), "{response}");
    }
    assert_eq!(state_of(&kb, id), ("non_teste".to_string(), None));
}

#[test]
fn p03_two_other_sessions_promote() {
    let kb = TestKb::new();
    let mut a = server_as(&kb, "agent:a");
    let mut b = server_as(&kb, "agent:b");
    let mut c = server_as(&kb, "agent:c");
    let created = mcp_call(
        &mut a,
        "kb_propose",
        json!({
            "product_id": "produit-test", "subject_key": "promotion", "kind": "piege",
            "title": "t", "correction": "c", "source": "agent", "terms": ["zorglub"],
            "identifiers": [{"identifier": "TAG_NOUVEAU", "state": "deprecie", "replaced_by": "TAG_ATTENTE"}]
        }),
    );
    let id = created["entry_id"].as_i64().expect("entry_id");

    let first = confirm_as(&mut b, id);
    assert!(error_code(&first).is_none(), "{first}");
    assert_eq!(state_of(&kb, id).0, "non_teste");
    // une même session qui confirme deux fois ne compte qu'une fois
    let again = confirm_as(&mut b, id);
    assert!(error_code(&again).is_none(), "{again}");
    assert_eq!(state_of(&kb, id).0, "non_teste");

    let second = confirm_as(&mut c, id);
    assert!(error_code(&second).is_none(), "{second}");
    assert_eq!(state_of(&kb, id), ("valide".to_string(), Some("auto".to_string())));
    let validated_by: String = kb
        .conn
        .query_row("SELECT validated_by FROM entries WHERE entry_id=?1", [id], |r| r.get(0))
        .unwrap();
    assert!(validated_by.starts_with("auto:"), "{validated_by}");

    let search = mcp_call(
        &mut c,
        "kb_search",
        json!({"product_id": "produit-test", "query": "zorglub"}),
    );
    assert_eq!(search["results"][0]["validation_mode"], "auto", "{search}");
    assert_eq!(search["covered"], true);

    let check = mcp_call(
        &mut c,
        "kb_check",
        json!({"product_id": "produit-test", "identifiers": ["TAG_NOUVEAU"]}),
    );
    assert_eq!(check["verdicts"][0]["verdict"], "deprecie", "{check}");
    assert_eq!(
        check["verdicts"][0]["entries_liees"][0]["validation_mode"],
        "auto",
        "{check}"
    );
}

#[test]
fn p04_confirm_requires_evidence() {
    let kb = TestKb::new();
    let mut a = server_as(&kb, "agent:a");
    let mut b = server_as(&kb, "agent:b");
    let id = propose_as(&mut a, "sans-preuve");
    let valid = confirm_as(&mut b, id);
    assert!(error_code(&valid).is_none(), "preuve valide refusée: {valid}");
    for args in [
        json!({"entry_id": id, "command": "", "exit_code": 0, "output_excerpt": "OK"}),
        json!({"entry_id": id, "command": "run", "exit_code": 0, "output_excerpt": "  "}),
        json!({"entry_id": id, "command": "run", "exit_code": 1, "output_excerpt": "KO"}),
        json!({"entry_id": id, "command": "run", "output_excerpt": "OK"}),
    ] {
        let response = mcp_call(&mut b, "kb_confirm", args);
        assert_eq!(error_code(&response), Some("invalid_argument"), "{response}");
    }
    assert_eq!(
        count(&kb, &format!("SELECT use_count FROM entries WHERE entry_id={id}")),
        1,
        "seule la confirmation valide est comptée"
    );
}

#[test]
fn p05_ko_resets_confirmations() {
    let kb = TestKb::new();
    let mut a = server_as(&kb, "agent:a");
    let id = propose_as(&mut a, "reset-ko");
    for actor in ["agent:b", "agent:c"] {
        let mut s = server_as(&kb, actor);
        confirm_as(&mut s, id);
    }
    assert_eq!(state_of(&kb, id).0, "valide");

    let mut d = server_as(&kb, "agent:d");
    let ko = mcp_call(
        &mut d,
        "kb_record_outcome",
        json!({"entry_id": id, "outcome": "ko", "note": "échec constaté"}),
    );
    assert!(error_code(&ko).is_none(), "{ko}");
    assert_eq!(state_of(&kb, id).0, "a_revoir");

    let mut e = server_as(&kb, "agent:e");
    confirm_as(&mut e, id);
    assert_eq!(state_of(&kb, id).0, "a_revoir", "les anciennes confirmations ne comptent plus");
    let mut f = server_as(&kb, "agent:f");
    confirm_as(&mut f, id);
    assert_eq!(state_of(&kb, id), ("valide".to_string(), Some("auto".to_string())));
}

#[test]
fn p06_content_update_resets_confirmations() {
    let kb = TestKb::new();
    let mut a = server_as(&kb, "agent:a");
    let id = propose_as(&mut a, "reset-contenu");
    let mut b = server_as(&kb, "agent:b");
    confirm_as(&mut b, id);

    let mut g = server_as(&kb, "agent:g");
    let update = mcp_call(
        &mut g,
        "kb_propose",
        json!({
            "product_id": "produit-test", "subject_key": "reset-contenu", "kind": "piege",
            "title": "titre v2", "correction": "correction v2", "source": "agent",
            "expected_version": 1
        }),
    );
    assert!(error_code(&update).is_none(), "{update}");

    let mut c = server_as(&kb, "agent:c");
    confirm_as(&mut c, id);
    assert_eq!(state_of(&kb, id).0, "non_teste", "la confirmation de b portait sur l'ancienne version");
    let mut d = server_as(&kb, "agent:d");
    confirm_as(&mut d, id);
    assert_eq!(state_of(&kb, id), ("valide".to_string(), Some("auto".to_string())));
}

#[test]
fn p07_review_and_human_validate() {
    let kb = TestKb::new();
    let cfg = manifest_dir().join("config");
    let mut a = server_as(&kb, "agent:a");
    let id1 = propose_as(&mut a, "revue-un");
    let id2 = propose_as(&mut a, "revue-deux");
    for id in [id1, id2] {
        for actor in ["agent:b", "agent:c"] {
            let mut s = server_as(&kb, actor);
            confirm_as(&mut s, id);
        }
    }
    let (code, out) = cli(&kb, &cfg, &["review", "--json"]);
    assert_eq!(code, 0, "{out}");
    let listed: Value = serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("{e}: {out}"));
    let ids: Vec<i64> = listed
        .as_array()
        .expect("tableau")
        .iter()
        .filter_map(|item| item["entry_id"].as_i64())
        .collect();
    assert!(ids.contains(&id1) && ids.contains(&id2), "{out}");

    let (code, out) = cli(
        &kb,
        &cfg,
        &["validate", &id1.to_string(), &id2.to_string(), "--by", "humain:test"],
    );
    assert_eq!(code, 0, "{out}");
    assert_eq!(state_of(&kb, id1), ("valide".to_string(), Some("humain".to_string())));
    assert_eq!(state_of(&kb, id2), ("valide".to_string(), Some("humain".to_string())));
    let (code, out) = cli(&kb, &cfg, &["review", "--json"]);
    assert_eq!(code, 0, "{out}");
    let listed: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 0, "{out}");
}

#[test]
fn p08_tool_list_has_no_human_action() {
    let names: Vec<String> = tool_definitions()
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    assert_eq!(names.len(), 11, "{names:?}");
    for name in &names {
        for forbidden in ["validate", "archive", "delete", "restore", "edit"] {
            assert!(!name.contains(forbidden), "{name}");
        }
    }
    for expected in ["kb_confirm", "kb_attach_repro", "kb_list_products", "kb_create_product", "kb_retract"] {
        assert!(names.contains(&expected.to_string()), "{expected} absent");
    }
}

#[test]
fn p09_search_prefix() {
    let kb = TestKb::new();
    let response = kb.search(&search_input("produit-test", "attend")).unwrap();
    assert!(
        response.results.iter().any(|r| r.subject_key == "attente-pause"),
        "{:?}",
        response.results
    );
}

#[test]
fn p10_search_infix_and_miss() {
    let kb = TestKb::new();
    let found = kb.search(&search_input("produit-test", "ttent")).unwrap();
    assert!(
        found.results.iter().any(|r| r.subject_key == "attente-pause"),
        "{:?}",
        found.results
    );
    let miss = kb.search(&search_input("produit-test", "xyzzy")).unwrap();
    assert!(miss.results.is_empty());
    assert!(!miss.covered);
}

#[test]
fn p11_version_segments() {
    let kb = TestKb::new();
    let cfg = temp_config(&[(
        "validation.yaml",
        "validation:\n  min_confirmations: 2\n  version_segments: 2\n".to_string(),
    )]);
    let (code, out) = cli(&kb, cfg.path(), &["product-version", "produit-test", "2.0.5"]);
    assert_eq!(code, 0, "{out}");
    let attente: String = kb
        .conn
        .query_row(
            "SELECT status FROM entries WHERE subject_key='attente-pause'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(attente, "valide", "2.0 et 2.0.5 coïncident sur 2 segments: {out}");
    let ancien: String = kb
        .conn
        .query_row("SELECT status FROM entries WHERE subject_key='ancien-param'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ancien, "a_revoir");
    let (code, out) = cli(&kb, cfg.path(), &["product-version", "produit-test", "2.1.0"]);
    assert_eq!(code, 0, "{out}");
    let attente: String = kb
        .conn
        .query_row(
            "SELECT status FROM entries WHERE subject_key='attente-pause'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(attente, "a_revoir");
}

fn repro_args(entry_id: i64, runner: &str) -> Value {
    json!({
        "entry_id": entry_id, "runner": runner, "script": "echo hi",
        "expected_exit": 0, "expected_contains": "kb"
    })
}

#[test]
fn p12_repro_attach_is_inert() {
    let kb = TestKb::new();
    let cfg = temp_config(&[(
        "runners.yaml",
        runners_yaml(&[("kbver", KB_EXE, &["--version"])]),
    )]);
    let config = AppConfig::load(Some(cfg.path())).unwrap();
    let mut server = McpServer::new(kb.open_second(), config).with_actor("agent:a");
    let id = propose_as(&mut server, "avec-repro");

    let refused = mcp_call(&mut server, "kb_attach_repro", repro_args(id, "bash"));
    assert_eq!(error_code(&refused), Some("invalid_argument"), "{refused}");

    let accepted = mcp_call(&mut server, "kb_attach_repro", repro_args(id, "kbver"));
    assert!(error_code(&accepted).is_none(), "{accepted}");
    assert_eq!(state_of(&kb, id).0, "non_teste");
    assert_eq!(count(&kb, "SELECT COUNT(*) FROM entry_outcomes"), 0);
    assert_eq!(count(&kb, "SELECT COUNT(*) FROM entry_repro"), 1);
}

fn setup_repro_entry(kb: &TestKb, server: &mut McpServer, subject: &str, runner: &str) -> i64 {
    let id = kb.insert_entry(
        "produit-test", subject, "piege", "t", "c", "a_revoir", None, Some("1.0"), &["zorglub"], &[],
    );
    let response = mcp_call(server, "kb_attach_repro", repro_args(id, runner));
    assert!(error_code(&response).is_none(), "{response}");
    id
}

#[test]
fn p13_revalidate_requires_approval() {
    let kb = TestKb::new();
    let cfg = temp_config(&[(
        "runners.yaml",
        runners_yaml(&[
            ("kbver", KB_EXE, &["--version"]),
            ("kbfail", KB_EXE, &["lint", "--option-inexistante"]),
        ]),
    )]);
    let config = AppConfig::load(Some(cfg.path())).unwrap();
    let mut server = McpServer::new(kb.open_second(), config).with_actor("agent:a");
    let ok_id = setup_repro_entry(&kb, &mut server, "rejeu-ok", "kbver");

    // sans approbation humaine : rien n'est exécuté
    let (code, out) = cli(&kb, cfg.path(), &["revalidate", "--product", "produit-test"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("non_approuve"), "{out}");
    assert_eq!(state_of(&kb, ok_id).0, "a_revoir");

    let (code, out) = cli(&kb, cfg.path(), &["approve-repro", &ok_id.to_string(), "--by", "humain:test"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = cli(&kb, cfg.path(), &["revalidate", "--product", "produit-test"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(state_of(&kb, ok_id), ("valide".to_string(), Some("auto".to_string())), "{out}");
    let (validated_by, version): (String, String) = kb
        .conn
        .query_row(
            "SELECT validated_by, product_version FROM entries WHERE entry_id=?1",
            [ok_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(validated_by.starts_with("rejeu:"), "{validated_by}");
    assert_eq!(version, "2.0");

    // rejeu en échec
    let fail_id = setup_repro_entry(&kb, &mut server, "rejeu-ko", "kbfail");
    let (code, out) = cli(&kb, cfg.path(), &["approve-repro", &fail_id.to_string(), "--by", "humain:test"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = cli(&kb, cfg.path(), &["revalidate", "--product", "produit-test"]);
    assert_ne!(code, 0, "un rejeu en échec doit produire un code de sortie non nul: {out}");
    assert_eq!(state_of(&kb, fail_id).0, "a_revoir");
    assert_eq!(
        count(&kb, &format!("SELECT fail_count FROM entries WHERE entry_id={fail_id}")),
        1
    );
}

#[test]
fn p14_missing_runner_program_is_explicit_error() {
    let kb = TestKb::new();
    let missing = if cfg!(windows) { "C:\\inexistant\\introuvable-runner.exe" } else { "/inexistant/introuvable-runner" };
    let cfg = temp_config(&[("runners.yaml", runners_yaml(&[("absent", missing, &[])]))]);
    let config = AppConfig::load(Some(cfg.path())).unwrap();
    let mut server = McpServer::new(kb.open_second(), config).with_actor("agent:a");
    let id = setup_repro_entry(&kb, &mut server, "rejeu-absent", "absent");
    let (code, out) = cli(&kb, cfg.path(), &["approve-repro", &id.to_string(), "--by", "humain:test"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = cli(&kb, cfg.path(), &["revalidate", "--product", "produit-test"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("introuvable-runner"), "{out}");
    assert_eq!(state_of(&kb, id).0, "a_revoir");
}

#[test]
fn p15_migration_old_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(&fs::read_to_string(manifest_dir().join("tests/fixtures/old_schema.sql")).unwrap())
            .unwrap();
        conn.execute_batch(
            "INSERT INTO products (product_id, name, current_version) VALUES ('p', 'p', '1.0');
             INSERT INTO entries (product_id, subject_key, kind, title, correction, status, source,
                                  created_at, updated_at)
             VALUES ('p', 'ancienne', 'piege', 'Instruction d''attente', 'Utiliser TAG_ATTENTE',
                     'non_teste', 'humain', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
             INSERT INTO entries_fts (rowid, title, symptom, cause, correction, terms)
             VALUES (1, 'Instruction d''attente', '', '', 'Utiliser TAG_ATTENTE', 'pause');",
        )
        .unwrap();
    }
    let conn = kb::db::open(&path).unwrap();
    kb::db::init(&conn).unwrap();
    let columns: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('entries')")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert!(columns.contains(&"validation_mode".to_string()), "{columns:?}");
    assert!(columns.contains(&"proposed_by".to_string()), "{columns:?}");
    for table in ["entry_evidence", "entry_repro"] {
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name=?1", [table], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "table {table} absente");
    }
    let response = kb::core::search::search(&conn, &search_input("p", "ttent")).unwrap();
    assert_eq!(response.results.len(), 1, "recherche par fragment après migration");
    let response = kb::core::search::search(&conn, &search_input("p", "pause")).unwrap();
    assert_eq!(response.results.len(), 1);
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
            out.push(path);
        }
    }
}

#[test]
fn p16_no_process_or_network_outside_repro() {
    let mut files = Vec::new();
    rust_files(&manifest_dir().join("src"), &mut files);
    let process_markers = ["Command::new", "std::process::Command", "tokio::process"];
    let network_markers = ["TcpListener", "TcpStream", "UdpSocket", "reqwest", "hyper::", "http://"];
    let mut spawners = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).unwrap();
        if process_markers.iter().any(|m| text.contains(m)) {
            spawners.push(file.file_name().unwrap().to_string_lossy().to_string());
        }
        for marker in network_markers {
            assert!(!text.contains(marker), "{} contient {marker}", file.display());
        }
    }
    assert_eq!(spawners, vec!["repro.rs".to_string()], "seul repro.rs peut lancer un processus");
    let server = fs::read_to_string(manifest_dir().join("src/mcp/server.rs")).unwrap();
    assert!(!server.contains("repro::run") && !server.contains("revalidate"));
}

// ---- P17 : hook d'adoption (PowerShell, Windows) ----

#[cfg(windows)]
fn run_hook(stdin_json: &Value) -> (i32, String, String) {
    let script = manifest_dir().join("hooks").join("kb_adoption.ps1");
    let mut child = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("powershell");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin_json.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[cfg(windows)]
fn transcript(dir: &Path, events: &[Value]) -> PathBuf {
    let path = dir.join("transcript.jsonl");
    let lines: Vec<String> = events.iter().map(|e| e.to_string()).collect();
    fs::write(&path, lines.join("\n")).unwrap();
    path
}

#[cfg(windows)]
fn user_msg() -> Value {
    json!({"type": "user", "message": {"role": "user", "content": "écris le script"}})
}

#[cfg(windows)]
fn tool_use(name: &str, input: Value) -> Value {
    json!({"type": "assistant", "message": {"role": "assistant",
        "content": [{"type": "tool_use", "name": name, "input": input}]}})
}

#[cfg(windows)]
#[test]
fn p17_hook_stop_blocks_without_kb_check() {
    let dir = tempfile::tempdir().unwrap();
    let path = transcript(
        dir.path(),
        &[user_msg(), tool_use("Write", json!({"file_path": "C:\\x\\flow.jsx", "content": "x"}))],
    );
    let (code, _out, err) = run_hook(&json!({
        "hook_event_name": "Stop", "transcript_path": path, "stop_hook_active": false
    }));
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("kb_check"), "{err}");
}

#[cfg(windows)]
#[test]
fn p17_hook_stop_passes_cases() {
    let dir = tempfile::tempdir().unwrap();
    // kb_check appelé après l'écriture
    let with_check = transcript(
        dir.path(),
        &[
            user_msg(),
            tool_use("Write", json!({"file_path": "C:\\x\\flow.jsx", "content": "x"})),
            tool_use("mcp__bcpe__kb_check", json!({"product_id": "p"})),
        ],
    );
    let (code, _o, err) = run_hook(&json!({"hook_event_name": "Stop", "transcript_path": with_check, "stop_hook_active": false}));
    assert_eq!(code, 0, "{err}");
    // pas de fichier script
    let notes = transcript(
        dir.path(),
        &[user_msg(), tool_use("Write", json!({"file_path": "C:\\x\\notes.txt", "content": "x"}))],
    );
    let (code, _o, err) = run_hook(&json!({"hook_event_name": "Stop", "transcript_path": notes, "stop_hook_active": false}));
    assert_eq!(code, 0, "{err}");
    // garde anti-boucle
    let unchecked = transcript(
        dir.path(),
        &[user_msg(), tool_use("Edit", json!({"file_path": "C:\\x\\flow.aml"}))],
    );
    let (code, _o, err) = run_hook(&json!({"hook_event_name": "Stop", "transcript_path": unchecked, "stop_hook_active": true}));
    assert_eq!(code, 0, "{err}");
}

#[cfg(windows)]
#[test]
fn p17_hook_post_tool_use_reminder() {
    let (code, out, err) = run_hook(&json!({
        "hook_event_name": "PostToolUse", "tool_name": "Write",
        "tool_input": {"file_path": "C:\\x\\flow.jsx"}
    }));
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("kb_check"), "{out}");
    let (code, out, err) = run_hook(&json!({
        "hook_event_name": "PostToolUse", "tool_name": "Write",
        "tool_input": {"file_path": "C:\\x\\notes.txt"}
    }));
    assert_eq!(code, 0, "{err}");
    assert!(out.trim().is_empty(), "{out}");
}

#[test]
fn p18_list_create_retract_and_graceful_unknown_product() {
    let kb = TestKb::new();
    let mut server = McpServer::new(kb.open_second(), AppConfig::load(None).unwrap()).with_actor("agent:mcp#123");

    // 1. kb_search & kb_check sur un produit inconnu : pas d'erreur bloquante
    let search_unk = mcp_call(&mut server, "kb_search", json!({"product_id": "inconnu-xyz", "query": "test"}));
    assert_eq!(search_unk["covered"], json!(false), "{search_unk}");
    assert_eq!(search_unk["product_status"], json!("unknown_product"));

    let check_unk = mcp_call(&mut server, "kb_check", json!({"product_id": "inconnu-xyz", "identifiers": ["fooBar"]}));
    assert_eq!(check_unk["product_status"], json!("unknown_product"), "{check_unk}");
    assert_eq!(check_unk["verdicts"][0]["verdict"], json!("inconnu"));

    // 2. kb_create_product : validation slug + created + already_exists
    let bad = mcp_call(&mut server, "kb_create_product", json!({"product_id": "Bad Slug!", "display_name": "Bad"}));
    assert_eq!(error_code(&bad), Some("invalid_argument"), "{bad}");

    let created = mcp_call(&mut server, "kb_create_product", json!({"product_id": "k4", "display_name": "vjoon K4"}));
    assert_eq!(created["status"], json!("created"), "{created}");

    let dup = mcp_call(&mut server, "kb_create_product", json!({"product_id": "k4", "display_name": "vjoon K4"}));
    assert_eq!(dup["status"], json!("already_exists"), "{dup}");

    // 3. kb_propose + kb_list_products + kb_retract
    let prop = mcp_call(&mut server, "kb_propose", json!({
        "product_id": "k4",
        "subject_key": "test-mistake",
        "kind": "piege",
        "title": "Fausse piste",
        "correction": "À rétracter",
        "source": "agent"
    }));
    let entry_id = prop["entry_id"].as_i64().unwrap();

    let list = mcp_call(&mut server, "kb_list_products", json!({}));
    let k4 = list["products"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["product_id"] == "k4")
        .unwrap();
    assert_eq!(k4["counts"]["non_teste"], json!(1));

    let ret = mcp_call(&mut server, "kb_retract", json!({"entry_id": entry_id, "reason": "Erreur agent"}));
    assert_eq!(ret["status"], json!("retracted"), "{ret}");
    assert_eq!(state_of(&kb, entry_id).0, "archive");
}

#[test]
fn p19_ingest_dedup_sanitizer_stats_export() {
    let kb = TestKb::new();
    let mut server = McpServer::new(kb.open_second(), AppConfig::load(None).unwrap()).with_actor("agent:mcp#456");

    mcp_call(&mut server, "kb_create_product", json!({"product_id": "indesign-server", "display_name": "Adobe InDesign Server"}));

    // 1. Auto-ingestion d'un fichier .d.ts / .ts dans le lexique
    let sdk_dir = tempfile::tempdir().unwrap();
    let dts = sdk_dir.path().join("api.d.ts");
    std::fs::write(&dts, "export interface Document {\n  saveACopy(dest: string): void;\n}\nexport function openProjectDocument(): Document;\n").unwrap();

    let mut conn = kb.open_second();
    let report = kb::core::lifecycle::ingest_lexicon(&mut conn, "indesign-server", sdk_dir.path()).unwrap();
    assert!(report["identifiers_added"].as_u64().unwrap() >= 2, "{report}");

    // 2. Sanitisation des secrets + détection de quasi-doublons dans kb_propose
    let p1 = mcp_call(&mut server, "kb_propose", json!({
        "product_id": "indesign-server",
        "subject_key": "save-copy-timeout",
        "kind": "piege",
        "title": "Erreur timeout lors de saveACopy sur serveur",
        "correction": "Utiliser Bearer secret_token_12345678 ou sk-proj-abcdef1234567890",
        "source": "agent"
    }));
    let id1 = p1["entry_id"].as_i64().unwrap();
    let full1 = mcp_call(&mut server, "kb_get", json!({"entry_id": id1}));
    let corr = full1["entry"]["correction"].as_str().unwrap();
    assert!(!corr.contains("secret_token_12345678"), "{corr}");
    assert!(!corr.contains("sk-proj-abcdef1234567890"), "{corr}");
    assert!(corr.contains("[REDACTED"), "{corr}");
    assert!(corr.contains("[REDACTED:API_KEY]"), "{corr}");

    let p2 = mcp_call(&mut server, "kb_propose", json!({
        "product_id": "indesign-server",
        "subject_key": "save-copy-timeout-v2",
        "kind": "piege",
        "title": "Timeout saveACopy serveur InDesign",
        "correction": "Autre contournement",
        "source": "agent"
    }));
    assert!(!p2["similar_entries"].as_array().unwrap().is_empty(), "{p2}");

    // 3. Stats & Export
    let stats = kb::core::lifecycle::kb_stats(&conn, Some("indesign-server")).unwrap();
    assert_eq!(stats["entries"]["non_teste"], json!(2));
    assert!(stats["lexicon_identifiers"].as_i64().unwrap() >= 2);

    let md = kb::core::lifecycle::export_product(&conn, "indesign-server", "md").unwrap();
    assert!(md.contains("save-copy-timeout"));
    assert!(md.contains("Adobe") || md.contains("indesign-server"));
}
