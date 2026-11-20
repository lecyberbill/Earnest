use wf_core::{generate_schema_json, parse_workflow_yaml, validate_workflow};

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
