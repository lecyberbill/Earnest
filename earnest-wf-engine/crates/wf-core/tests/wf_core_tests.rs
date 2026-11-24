use std::collections::HashMap;
use wf_core::{execute_workflow, generate_schema_json, parse_workflow_yaml, validate_workflow, ExecutionOptions};

const SAMPLE_WORKFLOW: &str = r#"
version: "0.1"
name: "sync_factures_fournisseurs"
description: "Récupère les PDF sur SFTP, archive sur SharePoint et notifie"
triggers:
  - type: cron
    expr: "0 30 7 * * 1-5"
    timezone: "Europe/Paris"
  - type: manual
connections:
  sftp_in:
    driver: sftp
    host: "sftp.fournisseur.fr"
    port: 22
    user: "{{ env.SFTP_USER }}"
    auth:
      kind: key_file
      path: "~/.ssh/id_ed25519"
  sp_archive:
    driver: sharepoint
    tenant_id: "{{ env.AZURE_TENANT_ID }}"
    client_id: "{{ env.AZURE_CLIENT_ID }}"
    client_secret: "{{ keyring.azure_sp_secret }}"
    site: "Finance"
    drive: "Documents"
  smtp_out:
    driver: smtp
    host: "smtp.office365.com"
    port: 587
    tls: starttls
    user: "{{ env.SMTP_USER }}"
    password: "{{ keyring.smtp_pass }}"
inputs:
  dossier_distant:
    type: string
    default: "/exports/factures"
vars:
  date_jour: "{{ now('%Y-%m-%d') }}"
steps:
  - id: list_remote
    name: "Lister les PDF distants"
    action: vfs.list
    params:
      conn: sftp_in
      path: "{{ inputs.dossier_distant }}"
      glob: "*.pdf"
    output: fichiers_pdf
    timeout_ms: 10000
    retry:
      max_attempts: 3
      backoff_ms: 1000
  - id: check_non_vide
    action: flow.if
    condition: "fichiers_pdf.data.len() > 0"
    then:
      - id: boucle_transfert
        action: flow.each
        items: "{{ fichiers_pdf.data }}"
        as: item
        concurrency: 2
        steps:
          - id: copie_sp
            action: vfs.transfer
            params:
              src: { conn: sftp_in, path: "{{ item.path }}" }
              dst: { conn: sp_archive, path: "/Factures/{{ vars.date_jour }}/{{ item.name }}" }
              overwrite: true
            on_error:
              - id: log_echec_fichier
                action: sys.log
                params:
                  level: error
                  message: "Echec transfert"
      - id: mail_synthese
        action: mail.send
        params:
          conn: smtp_out
          from: "automate@entreprise.fr"
          to: ["compta@entreprise.fr"]
          subject: "Factures archivées"
          body_text: "OK"
"#;

#[test]
fn parses_and_validates_reference_workflow_deterministically() {
    let spec = parse_workflow_yaml(SAMPLE_WORKFLOW).expect("sample workflow should parse");
    let report1 = validate_workflow(&spec);
    let report2 = validate_workflow(&spec);
    assert!(report1.valid, "unexpected errors: {:?}", report1.errors);
    assert_eq!(report1.step_count, 6);
    assert_eq!(report1.sha256, report2.sha256);
    assert_eq!(report1.sha256.len(), 64);
}

#[test]
fn rejects_unknown_yaml_fields_strictly() {
    let bad = r#"
version: "0.1"
name: "bad"
unknown_top_field: 123
steps:
  - id: s1
    action: sys.log
    params:
      message: "hi"
"#;
    assert!(parse_workflow_yaml(bad).is_err());
}

#[test]
fn rejects_duplicate_step_id_and_unknown_connection() {
    let bad = r#"
version: "0.1"
name: "bad_semantic"
connections:
  local_fs:
    driver: local
    root: "."
steps:
  - id: dup_id
    action: vfs.list
    params:
      conn: missing_conn
      path: "."
  - id: branch
    action: flow.if
    condition: "true"
    then:
      - id: dup_id
        action: sys.log
        params:
          message: "hi"
"#;
    let spec = parse_workflow_yaml(bad).unwrap();
    let report = validate_workflow(&spec);
    assert!(!report.valid);
    assert!(report.errors.iter().any(|e| e.code == "UNKNOWN_CONNECTION"));
    assert!(report.errors.iter().any(|e| e.code == "DUPLICATE_STEP_ID"));
}

#[test]
fn generates_valid_json_schema() {
    let json = generate_schema_json();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["title"], "WorkflowSpec");
}

#[test]
fn runtime_executes_local_vfs_if_each_and_dry_run() {
    let tmp = std::env::temp_dir().join(format!("wf_core_rt_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("in")).unwrap();
    std::fs::write(tmp.join("in/a.txt"), "hello").unwrap();
    std::fs::write(tmp.join("in/b.txt"), "world").unwrap();

    let yaml = format!(
        r#"
version: "0.1"
name: "local_copy"
connections:
  local_fs:
    driver: local
    root: "{}"
steps:
  - id: list_in
    action: vfs.list
    params:
      conn: local_fs
      path: "in"
      glob: "*.txt"
    output: files
  - id: check_non_empty
    action: flow.if
    condition: "files.data.len() > 0"
    then:
      - id: loop_files
        action: flow.each
        items: "{{{{ files.data }}}}"
        as: item
        steps:
          - id: copy_one
            action: vfs.transfer
            params:
              src: {{ conn: local_fs, path: "in/{{{{ item.name }}}}" }}
              dst: {{ conn: local_fs, path: "out/{{{{ item.name }}}}" }}
              overwrite: true
"#,
        tmp.to_string_lossy().replace('\\', "/")
    );

    let spec = parse_workflow_yaml(&yaml).unwrap();

    // 1. Dry-run ne doit pas écrire sur disque
    let dry = execute_workflow(
        &spec,
        ExecutionOptions {
            dry_run: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(dry.success);
    assert!(!tmp.join("out/a.txt").exists());

    // 2. Exécution réelle copie a.txt et b.txt
    let real = execute_workflow(&spec, ExecutionOptions::default()).unwrap();
    assert!(real.success, "error: {:?}", real.error);
    assert_eq!(std::fs::read_to_string(tmp.join("out/a.txt")).unwrap(), "hello");
    assert_eq!(std::fs::read_to_string(tmp.join("out/b.txt")).unwrap(), "world");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn runtime_handles_on_error_and_redacts_secrets() {
    let yaml = r#"
version: "0.1"
name: "error_recovery"
connections:
  local_fs:
    driver: local
    root: "."
steps:
  - id: will_fail
    action: flow.fail
    params:
      code: "E_TEST"
      message: "bad token super_secret_token_xyz"
    on_error:
      - id: log_err
        action: sys.log
        params:
          level: error
          message: "Recovered: {{ error.message }}"
"#;
    let spec = parse_workflow_yaml(yaml).unwrap();
    let mut secrets = HashMap::new();
    secrets.insert("api_key".to_string(), "super_secret_token_xyz".to_string());
    let report = execute_workflow(
        &spec,
        ExecutionOptions {
            secrets,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(report.success);
    assert_eq!(report.logs[0].status, "failed");
    assert!(!report.logs[0].error.as_ref().unwrap().contains("super_secret_token_xyz"));
    assert_eq!(report.logs[1].step_id, "log_err");
    assert_eq!(report.logs[1].status, "success");
}

#[test]
fn dry_run_vfs_overlay_supports_chained_write_read_transfer_stat_delete() {
    let tmp = std::env::temp_dir().join(format!("wf_core_dry_vfs_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let yaml = format!(
        r#"
version: "0.1"
name: "dry_vfs_chain"
connections:
  local_fs:
    driver: local
    root: "{}"
steps:
  - id: w
    action: vfs.write
    params:
      conn: local_fs
      path: "virtual.txt"
      content: "hello dry vfs"
  - id: r
    action: vfs.read
    params:
      conn: local_fs
      path: "virtual.txt"
    output: rd
  - id: cp
    action: vfs.transfer
    params:
      src: {{ conn: local_fs, path: "virtual.txt" }}
      dst: {{ conn: local_fs, path: "virtual_copy.txt" }}
  - id: st
    action: vfs.stat
    params:
      conn: local_fs
      path: "virtual_copy.txt"
      compute_sha256: true
  - id: del
    action: vfs.delete
    params:
      conn: local_fs
      path: "virtual.txt"
"#,
        tmp.to_string_lossy().replace('\\', "/")
    );

    let spec = parse_workflow_yaml(&yaml).unwrap();
    let report = execute_workflow(
        &spec,
        ExecutionOptions {
            dry_run: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(report.success, "dry_run chain failed: {:?}", report.error);
    assert_eq!(report.logs[1].output.as_ref().unwrap()["content"], "hello dry vfs");
    assert_eq!(report.logs[3].output.as_ref().unwrap()["size"], 13);
    assert!(report.logs[3].output.as_ref().unwrap()["sha256"].is_string());
    assert!(!tmp.join("virtual.txt").exists());
    assert!(!tmp.join("virtual_copy.txt").exists());

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn runtime_executes_sys_exec_in_dry_run_and_real_mode() {
    let prog = if cfg!(windows) { "cmd" } else { "echo" };
    let args = if cfg!(windows) {
        r#"["/C", "echo", "hello_exec"]"#
    } else {
        r#"["hello_exec"]"#
    };
    let yaml = format!(
        r#"
version: "0.1"
name: "exec_test"
steps:
  - id: run_cmd
    action: sys.exec
    params:
      program: "{}"
      args: {}
"#,
        prog, args
    );
    let spec = parse_workflow_yaml(&yaml).unwrap();
    let dry = execute_workflow(
        &spec,
        ExecutionOptions {
            dry_run: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(dry.success);
    assert_eq!(dry.logs[0].output.as_ref().unwrap()["dry_run"], true);

    let real = execute_workflow(&spec, ExecutionOptions::default()).unwrap();
    assert!(real.success);
    let out = real.logs[0].output.as_ref().unwrap();
    assert_eq!(out["exit_code"], 0);
    assert!(out["stdout"].as_str().unwrap().contains("hello_exec"));
}

#[test]
fn runtime_executes_sys_http_with_connection_and_dry_run() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let body = r#"{"ok":true,"msg":"pong"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    let yaml = format!(
        r#"
version: "0.1"
name: "http_test"
connections:
  api:
    driver: http
    base_url: "http://{}"
    headers:
      X-Client: "earnest-wf"
steps:
  - id: call_ping
    action: sys.http
    params:
      conn: api
      method: POST
      url: "/ping"
      query:
        v: "1"
      body:
        hello: "world"
"#,
        addr
    );

    let spec = parse_workflow_yaml(&yaml).unwrap();
    let dry = execute_workflow(
        &spec,
        ExecutionOptions {
            dry_run: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(dry.success);
    assert_eq!(dry.logs[0].output.as_ref().unwrap()["dry_run"], true);

    let real = execute_workflow(&spec, ExecutionOptions::default()).unwrap();
    assert!(real.success, "http error: {:?}", real.error);
    let out = real.logs[0].output.as_ref().unwrap();
    assert_eq!(out["status"], 200);
    assert_eq!(out["body"]["msg"], "pong");

    let _ = handle.join();
}

#[test]
fn debug_session_and_store_folders_enabled_runs_work() {
    let tmp = std::env::temp_dir().join(format!("wf_core_dbg_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("workflows")).unwrap();

    let yaml = r#"
version: "0.1"
name: "dbg_flow"
steps:
  - id: s1
    action: sys.log
    params:
      message: "step1"
  - id: s2
    action: sys.log
    params:
      message: "step2"
"#;
    std::fs::write(tmp.join("workflows/dbg_flow.wf.yaml"), yaml).unwrap();
    let store = wf_core::WfStore::open(&tmp.join(".wf/wf.db")).unwrap();
    assert_eq!(store.scan_workflows_dir(&tmp.join("workflows")).unwrap(), 1);
    store.set_workflow_folder("dbg_flow", "Prod/Finance").unwrap();
    store.set_workflow_cron("dbg_flow", Some("0 2 * * *")).unwrap();
    store.seal_workflow("dbg_flow").unwrap();
    store.set_workflow_enabled("dbg_flow", false).unwrap();
    assert!(store
        .execute_and_record("dbg_flow", ExecutionOptions::default())
        .is_err());
    store.set_workflow_enabled("dbg_flow", true).unwrap();
    let (_rid, _rep) = store
        .execute_and_record("dbg_flow", ExecutionOptions::default())
        .unwrap();
    assert_eq!(store.list_runs(Some("dbg_flow"), 10).unwrap().len(), 1);

    let spec = parse_workflow_yaml(yaml).unwrap();
    let mut dbg = wf_core::DebugSession::new(
        spec,
        ExecutionOptions {
            dry_run: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(dbg.current_step_id(), Some("s1"));
    assert_eq!(dbg.step().unwrap().step_id, "s1");
    assert_eq!(dbg.current_step_id(), Some("s2"));
    assert_eq!(dbg.step().unwrap().step_id, "s2");
    assert!(dbg.is_finished());

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn wfl_v1_rich_list_pipes_loops_data_and_vfs_ops() {
    let tmp = std::env::temp_dir().join("wf_core_v1_test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("sub")).unwrap();
    std::fs::write(tmp.join("alpha.txt"), "hello").unwrap();
    std::fs::write(tmp.join("sub").join("beta_2026.pdf"), "pdf").unwrap();

    let yaml = format!(
        r#"
version: "1.0"
name: "wfl_rich_test"
connections:
  local:
    driver: local
    root: "{}"
vars:
  today: "${{now('%Y')}}"
  counter: 0
  stems_acc: []
steps:
  - id: list_all
    action: vfs.list
    params:
      conn: local
      path: "."
      recursive: true
    output: files
  - id: each_file
    action: flow.each
    items: "${{vars.files.data}}"
    as: f
    steps:
      - id: only_pdf_or_txt
        action: flow.if
        condition: "${{f.ext}} == 'txt' || (${{f.ext}} == 'pdf' && ${{f.stem}} contains 'beta')"
        then:
          - id: add_stem
            action: flow.append
            params:
              var: stems_acc
              value: "${{f.stem | upper}}"
          - id: append_log
            action: vfs.append
            params:
              conn: local
              path: "summary.txt"
              content: "${{loop.index1}}:${{f.stem}}:${{vars.today}}"
  - id: loop_while
    action: flow.while
    condition: "${{vars.counter}} < 5"
    max_iterations: 20
    steps:
      - id: inc
        action: flow.set
        params:
          var: counter
          value: "${{vars.counter}} + 1"
      - id: stop_at_3
        action: flow.if
        condition: "${{vars.counter}} == 3"
        then:
          - id: brk
            action: flow.break
  - id: sw
    action: flow.switch
    value: "${{vars.counter}}"
    cases:
      "3":
        - id: sw_ok
          action: flow.set
          params:
            var: sw_res
            value: "matched_3"
    default: []
  - id: csv_step
    action: data.parse_csv
    params:
      text: "name,score\nalice,10\nbob,20"
    output: csv_out
  - id: regex_step
    action: data.regex
    params:
      text: "ID-42-OK"
      pattern: "ID-(\\d+)"
    output: re_out
  - id: mk
    action: vfs.mkdir
    params:
      conn: local
      path: "out_dir"
  - id: ex
    action: vfs.exists
    params:
      conn: local
      path: "summary.txt"
    output: ex_out
"#,
        tmp.to_string_lossy().replace('\\', "/")
    );

    let spec = parse_workflow_yaml(&yaml).unwrap();
    let rep = execute_workflow(
        &spec,
        ExecutionOptions {
            dry_run: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(rep.success, "workflow failed: {:?}", rep.error);

    let summary = std::fs::read_to_string(tmp.join("summary.txt")).unwrap();
    assert!(summary.contains("alpha"));
    assert!(summary.contains("beta_2026"));
    assert!(tmp.join("out_dir").is_dir());

    let ex_log = rep.logs.iter().find(|l| l.step_id == "ex").unwrap();
    assert_eq!(ex_log.output.as_ref().unwrap()["exists"], true);

    let csv_log = rep.logs.iter().find(|l| l.step_id == "csv_step").unwrap();
    assert_eq!(csv_log.output.as_ref().unwrap()["count"], 2);

    let re_log = rep.logs.iter().find(|l| l.step_id == "regex_step").unwrap();
    assert_eq!(re_log.output.as_ref().unwrap()["first"]["groups"][0], "42");

    let _ = std::fs::remove_dir_all(&tmp);
}
