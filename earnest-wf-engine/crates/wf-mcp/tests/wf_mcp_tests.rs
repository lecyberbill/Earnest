use serde_json::json;
use wf_mcp::WfMcpServer;

#[test]
fn end_to_end_mcp_draft_dry_run_seal_and_real_execution() {
    let tmp = std::env::temp_dir().join(format!("wf_mcp_e2e_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("in")).unwrap();
    std::fs::write(tmp.join("in/doc.txt"), "payload").unwrap();

    let server = WfMcpServer::open(&tmp, tmp.join("wf.db")).unwrap();

    // 1. initialize & tools/list
    let init = server
        .handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}))
        .unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "wf-mcp");
    let list = server
        .handle(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
        .unwrap();
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 11);

    // 2. wfl_get_grammar
    let gram = server.call_tool("wfl_get_grammar", &json!({})).unwrap();
    assert_eq!(gram["schema"]["title"], "WorkflowSpec");

    // 3. wfl_validate + wfl_save_draft
    let yaml = format!(
        r#"
version: "0.1"
name: "copy_doc"
connections:
  local_fs:
    driver: local
    root: "{}"
steps:
  - id: cp
    action: vfs.transfer
    params:
      src: {{ conn: local_fs, path: "in/doc.txt" }}
      dst: {{ conn: local_fs, path: "out/doc.txt" }}
"#,
        tmp.to_string_lossy().replace('\\', "/")
    );
    let val = server.call_tool("wfl_validate", &json!({"yaml": yaml})).unwrap();
    assert_eq!(val["valid"], true);

    let draft = server.call_tool("wfl_save_draft", &json!({"yaml": yaml})).unwrap();
    assert_eq!(draft["status"], "draft");

    // 4. Real run rejected while draft
    assert!(server
        .call_tool("wfl_run", &json!({"name": "copy_doc", "dry_run": false}))
        .is_err());

    // 5. Dry-run succeeds without touching disk
    let dry = server
        .call_tool("wfl_run", &json!({"name": "copy_doc", "dry_run": true}))
        .unwrap();
    assert_eq!(dry["report"]["success"], true);
    assert!(!tmp.join("out/doc.txt").exists());

    // 6. Seal -> real run executes and persists in SQLite WAL
    let sealed = server.call_tool("wfl_seal", &json!({"name": "copy_doc"})).unwrap();
    assert_eq!(sealed["status"], "sealed");

    let real = server
        .call_tool("wfl_run", &json!({"name": "copy_doc", "dry_run": false}))
        .unwrap();
    assert_eq!(real["report"]["success"], true);
    assert_eq!(std::fs::read_to_string(tmp.join("out/doc.txt")).unwrap(), "payload");

    // 7. Inspect run in SQLite
    let run_id = real["run_id"].as_str().unwrap();
    let inspected = server
        .call_tool("wfl_inspect_run", &json!({"run_id": run_id}))
        .unwrap();
    assert_eq!(inspected["found"], true);
    assert_eq!(inspected["run"]["status"], "success");
    assert_eq!(inspected["run"]["step_logs"].as_array().unwrap().len(), 1);

    // 8. Folders, enable/disable, cron & list_runs
    server
        .call_tool("wfl_set_folder", &json!({"name": "copy_doc", "folder": "Prod"}))
        .unwrap();
    server
        .call_tool("wfl_set_cron", &json!({"name": "copy_doc", "cron_expr": "0 2 * * *"}))
        .unwrap();
    server
        .call_tool("wfl_set_enabled", &json!({"name": "copy_doc", "enabled": false}))
        .unwrap();
    assert!(server
        .call_tool("wfl_run", &json!({"name": "copy_doc", "dry_run": false}))
        .is_err());
    let runs = server
        .call_tool("wfl_list_runs", &json!({"workflow_name": "copy_doc", "limit": 10}))
        .unwrap();
    assert_eq!(runs["runs"].as_array().unwrap().len(), 2); // dry_run + real

    let _ = std::fs::remove_dir_all(&tmp);
}
