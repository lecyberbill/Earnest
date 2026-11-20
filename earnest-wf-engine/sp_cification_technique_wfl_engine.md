# Spécification Technique : WFL Engine & Studio (`wf-engine`)

**Version :** `0.1.0-alpha`  
**Cible :** Rust (Edition 2021/2024)  
**Intégration :** Earnest Operations Center (MCP & IPC), AORUI (GPU/wgpu Studio), LLM Locaux (`llama-server` / Ollama)

---

## 1. Vision & Positionnement Architectural

**WFL (Workflow Language)** est un moteur d'orchestration généraliste, déterministe et léger, conçu pour remplacer les moteurs propriétaires lourds (type Fortra Automate / AML) et servir de bras armé fiable aux agents IA d'**Earnest Operations Center**.

### Principes Fondamentaux (Invariants)
1. **Lisible par l'Humain, Déterministe pour la Machine :** Les workflows sont décrits en YAML déclaratif strict (`#[serde(deny_unknown_fields)]`). Un opérateur humain peut relire, auditer ou éditer chaque flux en quelques secondes.
2. **Séparation stricte Conception IA / Exécution Déterministe :** Le LLM (local ou agent Earnest) ne *réalise* jamais les opérations système directement ; il **compile** l'intention en fichier `.wf.yaml`, qui est ensuite validé statiquement puis exécuté de façon déterministe par le runtime Rust.
3. **Gouvernance & Scellement (Earnest Governance) :** Tout workflow validé peut être scellé via empreinte **SHA-256** (`manifest.lock`). `wf-daemon` refuse d'exécuter en production un workflow modifié sans revalidation ou marqué `draft` (exécution forcée en `--dry-run`).
4. **Exécution Décorrélée de l'Interface :** Les exécutions (`runs`) et déclencheurs (`cron`, `vfs_watch`, `mail_watch`) tournent dans un démon système léger (`wf-daemon`) sans dépendance GPU. L'interface graphique **AORUI** (`wf-ui`) se connecte à la demande via IPC + SQLite WAL.
5. **Zéro Fuite de Secrets :** Les identifiants ne sont jamais écrits en clair dans les workflows ni exposés au contexte des LLM. Ils sont résolus à l'exécution depuis le coffre-fort OS (`keyring`) ou les variables d'environnement (`env`).

---

## 2. Topologie du Workspace Cargo

```text
wf-engine/
├── Cargo.toml                  # Workspace Cargo
├── schemas/
│   └── wfl-v0.1.schema.json    # Généré automatiquement via schemars
└── crates/
    ├── wf-core/                # AST YAML, Validateur statique, Moteur Tokio, OpenDAL, Lettre, SQLite WAL
    ├── wf-ai/                  # Client LLM local (/v1/chat/completions) + Boucle d'auto-réparation YAML
    ├── wf-daemon/              # Service OS arrière-plan (Scheduler CRON, Watchers, Serveur IPC)
    ├── wf-mcp/                 # Serveur MCP (stdio/SSE) pour pilotage par Earnest Operations Center
    └── wf-ui/                  # Studio Desktop natif propulsé par AORUI (wgpu / SDF / Taffy)
```

### Dépendances Rust Clés (`Cargo.toml` workspace)
* **Sérialisation & Schéma :** `serde`, `serde_json`, `serde_yaml_ng`, `schemars`
* **Runtime & Concurrence :** `tokio` (full), `async-trait`, `futures`
* **Templating & Expressions :** `minijinja` (interpolation `{{ ... }}`), `rhai` (évaluation sandboxée des `condition:`)
* **Connecteurs Unifiés (VFS) :** `opendal` (features : `services-fs`, `services-sftp`, `services-ftp`, `services-s3`, `services-gcs`, `services-gdrive`, `services-dropbox`, `services-onedrive`, `services-http`)
* **Messagerie :** `lettre` (SMTP/MIME), `async-imap` (IMAP watcher/fetch)
* **Persistance & Sécurité :** `sqlx` (SQLite WAL), `keyring` (coffre-fort OS), `sha2`, `cron`, `chrono`
* **IPC & MCP :** `interprocess` (Named Pipes Windows / Unix Domain Sockets)

---

## 3. Spécification du Langage YAML (`WFL v0.1`)

### 3.1 Anatomie d'un fichier `.wf.yaml`

```yaml
version: "0.1"
name: "sync_factures_fournisseurs"
description: "Récupère les PDF sur SFTP, archive sur SharePoint et notifie la compta"

# Déclencheurs gérés en tâche de fond par wf-daemon
triggers:
  - type: cron
    expr: "0 30 7 * * 1-5"
    timezone: "Europe/Paris"
  - type: manual

# Connexions logiques (les secrets sont résolus au runtime, jamais exposés aux LLM)
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

# Contrat d'entrée typé
inputs:
  dossier_distant:
    type: string
    default: "/exports/factures"
  dry_run:
    type: bool
    default: false

# Variables locales du contexte d'exécution
vars:
  date_jour: "{{ now('%Y-%m-%d') }}"

# Étapes séquentielles / arborescentes
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
                  message: "Échec transfert {{ item.name }} : {{ error.message }}"

      - id: mail_synthese
        action: mail.send
        params:
          conn: smtp_out
          from: "automate@entreprise.fr"
          to: ["compta@entreprise.fr"]
          subject: "[Auto] {{ fichiers_pdf.data | length }} factures archivées"
          body_text: "Transfert terminé vers /Factures/{{ vars.date_jour }}."
    else:
      - id: log_rien
        action: sys.log
        params:
          level: info
          message: "Aucun fichier à traiter aujourd'hui."
```

### 3.2 Référentiel des Primitives (`ActionSpec`)

| Domaine | Action | Paramètres obligatoires / optionnels | Sortie `ActionResult.data` |
| :--- | :--- | :--- | :--- |
| **VFS** | `vfs.list` | `conn`, `path`, `glob?`, `recursive?` | `[{ path, name, size, modified_at }]` |
| | `vfs.read` | `conn`, `path`, `encoding?` (`utf8` \| `base64`) | `{ content, size }` |
| | `vfs.write` | `conn`, `path`, `content`, `create_dirs?` | `{ path, bytes_written }` |
| | `vfs.transfer` | `src: {conn, path}`, `dst: {conn, path}`, `overwrite?` | `{ bytes_transferred, sha256 }` |
| | `vfs.delete` | `conn`, `path` | `{ deleted: bool }` |
| | `vfs.stat` | `conn`, `path`, `compute_sha256?` | `{ exists, size, sha256? }` |
| **MAIL** | `mail.send` | `conn`, `from`, `to[]`, `cc[]?`, `subject`, `body_text?`, `body_html?`, `attachments[]?` | `{ message_id }` |
| | `mail.fetch` | `conn`, `folder?`, `limit?`, `unread_only?`, `mark_read?` | `[{ uid, from, subject, date, text }]` |
| **FLOW** | `flow.if` | `condition` (expr Rhai), `then[]`, `else[]?` | Sortie de la branche exécutée |
| | `flow.each` | `items`, `as`, `concurrency?`, `steps[]` | `[ActionResult.data]` |
| | `flow.set` | `var`, `value` | `{ value }` |
| | `flow.sleep` | `duration_ms` | `{ slept_ms }` |
| | `flow.call` | `workflow_path`, `inputs?` | Outputs du sous-workflow |
| | `flow.fail` | `code`, `message` | Interrompt le run avec erreur |
| **SYS** | `sys.exec` | `program`, `args[]?`, `cwd?`, `env?`, `allow_non_zero?` | `{ exit_code, stdout, stderr }` |
| | `sys.http` | `conn?`, `method`, `url`, `headers?`, `query?`, `body?`, `parse_json?` | `{ status, headers, body }` |
| | `sys.log` | `level` (`debug`\|`info`\|`warn`\|`error`), `message` | `{ logged: true }` |

---

## 4. Structures AST Rust (`crates/wf-core/src/ast.rs`)

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowSpec {
    pub version: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub triggers: Vec<TriggerSpec>,
    #[serde(default)]
    pub connections: HashMap<String, ConnectionSpec>,
    #[serde(default)]
    pub inputs: HashMap<String, InputParamSpec>,
    #[serde(default)]
    pub vars: HashMap<String, Value>,
    pub steps: Vec<StepSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TriggerSpec {
    Manual,
    Cron {
        expr: String,
        #[serde(default = "default_timezone")]
        timezone: String,
    },
    VfsWatch {
        conn: String,
        path: String,
        #[serde(default)]
        glob: Option<String>,
        #[serde(default = "default_poll_ms")]
        poll_interval_ms: u64,
    },
    MailWatch {
        conn: String,
        #[serde(default = "default_inbox")]
        folder: String,
        #[serde(default = "default_true")]
        unread_only: bool,
        #[serde(default)]
        subject_regex: Option<String>,
        #[serde(default = "default_poll_ms")]
        poll_interval_ms: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "driver", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConnectionSpec {
    Local { root: String },
    Sftp {
        host: String,
        #[serde(default = "default_sftp_port")]
        port: u16,
        user: String,
        auth: SftpAuthSpec,
    },
    Ftp {
        host: String,
        #[serde(default = "default_ftp_port")]
        port: u16,
        user: String,
        password: String,
        #[serde(default)]
        tls: bool,
    },
    S3 {
        bucket: String,
        region: String,
        #[serde(default)]
        endpoint: Option<String>,
        access_key: String,
        secret_key: String,
    },
    Gcs {
        bucket: String,
        service_account_json: String,
    },
    Sharepoint {
        tenant_id: String,
        client_id: String,
        client_secret: String,
        site: String,
        #[serde(default = "default_drive")]
        drive: String,
    },
    Gdrive {
        client_id: String,
        client_secret: String,
        refresh_token: String,
        #[serde(default)]
        root_folder_id: Option<String>,
    },
    Dropbox { access_token: String },
    Smtp {
        host: String,
        #[serde(default = "default_smtp_port")]
        port: u16,
        #[serde(default)]
        tls: SmtpTlsMode,
        user: String,
        password: String,
    },
    Imap {
        host: String,
        #[serde(default = "default_imap_port")]
        port: u16,
        #[serde(default = "default_true")]
        tls: bool,
        user: String,
        password: String,
    },
    Http {
        base_url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SftpAuthSpec {
    Password { password: String },
    KeyFile { path: String, #[serde(default)] passphrase: Option<String> },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SmtpTlsMode {
    None,
    #[default]
    Starttls,
    Implicit,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InputParamSpec {
    #[serde(rename = "type")]
    pub param_type: ParamType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ParamType { String, Int, Float, Bool, List, Object }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepSpec {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(flatten)]
    pub action: ActionSpec,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub retry: Option<RetryPolicySpec>,
    #[serde(default)]
    pub on_error: Vec<StepSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RetryPolicySpec {
    pub max_attempts: u32,
    #[serde(default = "default_backoff_ms")]
    pub backoff_ms: u64,
    #[serde(default = "default_true")]
    pub exponential: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action")]
pub enum ActionSpec {
    #[serde(rename = "vfs.list")]
    VfsList { params: VfsListParams },
    #[serde(rename = "vfs.read")]
    VfsRead { params: VfsReadParams },
    #[serde(rename = "vfs.write")]
    VfsWrite { params: VfsWriteParams },
    #[serde(rename = "vfs.transfer")]
    VfsTransfer { params: VfsTransferParams },
    #[serde(rename = "vfs.delete")]
    VfsDelete { params: VfsLocation },
    #[serde(rename = "vfs.stat")]
    VfsStat { params: VfsStatParams },

    #[serde(rename = "mail.send")]
    MailSend { params: MailSendParams },
    #[serde(rename = "mail.fetch")]
    MailFetch { params: MailFetchParams },

    #[serde(rename = "flow.if")]
    FlowIf {
        condition: String,
        then: Vec<StepSpec>,
        #[serde(rename = "else", default)]
        else_branch: Vec<StepSpec>,
    },
    #[serde(rename = "flow.each")]
    FlowEach {
        items: String,
        #[serde(rename = "as")]
        item_var: String,
        #[serde(default = "default_concurrency")]
        concurrency: usize,
        steps: Vec<StepSpec>,
    },
    #[serde(rename = "flow.set")]
    FlowSet { params: FlowSetParams },
    #[serde(rename = "flow.sleep")]
    FlowSleep { params: FlowSleepParams },
    #[serde(rename = "flow.call")]
    FlowCall { params: FlowCallParams },
    #[serde(rename = "flow.fail")]
    FlowFail { params: FlowFailParams },

    #[serde(rename = "sys.exec")]
    SysExec { params: SysExecParams },
    #[serde(rename = "sys.http")]
    SysHttp { params: SysHttpParams },
    #[serde(rename = "sys.log")]
    SysLog { params: SysLogParams },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VfsLocation { pub conn: String, pub path: String }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VfsListParams {
    pub conn: String,
    pub path: String,
    #[serde(default)]
    pub glob: Option<String>,
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VfsReadParams {
    pub conn: String,
    pub path: String,
    #[serde(default)]
    pub encoding: ReadEncoding,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadEncoding { #[default] Utf8, Base64 }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VfsWriteParams {
    pub conn: String,
    pub path: String,
    pub content: String,
    #[serde(default = "default_true")]
    pub create_dirs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VfsTransferParams {
    pub src: VfsLocation,
    pub dst: VfsLocation,
    #[serde(default = "default_true")]
    pub overwrite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VfsStatParams {
    pub conn: String,
    pub path: String,
    #[serde(default)]
    pub compute_sha256: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailSendParams {
    pub conn: String,
    pub from: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    pub subject: String,
    #[serde(default)]
    pub body_text: Option<String>,
    #[serde(default)]
    pub body_html: Option<String>,
    #[serde(default)]
    pub attachments: Vec<VfsLocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailFetchParams {
    pub conn: String,
    #[serde(default = "default_inbox")]
    pub folder: String,
    #[serde(default = "default_fetch_limit")]
    pub limit: usize,
    #[serde(default = "default_true")]
    pub unread_only: bool,
    #[serde(default)]
    pub mark_read: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowSetParams { pub var: String, pub value: Value }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowSleepParams { pub duration_ms: u64 }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowCallParams {
    pub workflow_path: String,
    #[serde(default)]
    pub inputs: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowFailParams { pub code: String, pub message: String }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SysExecParams {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub allow_non_zero: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SysHttpParams {
    #[serde(default)]
    pub conn: Option<String>,
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub query: HashMap<String, String>,
    #[serde(default)]
    pub body: Option<Value>,
    #[serde(default = "default_true")]
    pub parse_json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SysLogParams {
    #[serde(default)]
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel { Debug, #[default] Info, Warn, Error }

fn default_timezone() -> String { "UTC".to_string() }
fn default_poll_ms() -> u64 { 30_000 }
fn default_inbox() -> String { "INBOX".to_string() }
fn default_drive() -> String { "Documents".to_string() }
fn default_sftp_port() -> u16 { 22 }
fn default_ftp_port() -> u16 { 21 }
fn default_smtp_port() -> u16 { 587 }
fn default_imap_port() -> u16 { 993 }
fn default_backoff_ms() -> u64 { 500 }
fn default_concurrency() -> usize { 1 }
fn default_fetch_limit() -> usize { 20 }
fn default_true() -> bool { true }
```

---

## 5. Intégration avec Earnest Operations Center (`wf-mcp`)

Le crate `wf-mcp` expose le moteur aux agents d'**Earnest Operations Center** sous forme d'outils MCP déterministes. L'agent peut ainsi créer un flux complexe, le faire valider, le lancer en simulation (`dry_run`), et soumettre le fichier `.wf.yaml` à la revue humaine.

### Outils MCP Exposés
1. `wfl_get_grammar` : Retourne le schéma JSON compact (`wfl-v0.1.schema.json`) et la liste des connexions déclarées dans le profil actif (noms et drivers uniquement, **sans secrets**).
2. `wfl_validate` : Prend un contenu YAML brut, exécute le parsing AST + l'analyse sémantique (unicité des IDs, existence des `conn:`, graphe des variables `output` → `{{ ... }}`) et retourne soit `VALID` + empreinte SHA-256, soit la liste exacte des erreurs à corriger.
3. `wfl_save_draft` : Écrit le YAML validé dans le répertoire des workflows avec le statut `draft` (non scellé).
4. `wfl_run` : Déclenche l'exécution via IPC sur `wf-daemon` (avec `dry_run: true` obligatoire tant que le workflow n'est pas scellé par un humain dans AORUI / Earnest).
5. `wfl_inspect_run` : Interroge SQLite WAL pour récupérer le statut étape par étape, les durées et les éventuels messages d'erreur d'un `run_id`.
6. `wfl_list_workflows` : Liste les workflows déployés, leur statut de scellement (`sealed` / `draft`), leurs triggers actifs et leur dernière exécution.

---

## 6. Contrat d'Exécution, Persistance SQLite WAL & IPC (`wf-daemon` ↔ `wf-ui`)

### 6.1 Schéma SQLite (`PRAGMA journal_mode = WAL;`)

```sql
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
    status TEXT NOT NULL CHECK(status IN ('running', 'success', 'failed', 'skipped')),
    started_at TEXT NOT NULL,
    duration_ms INTEGER,
    output_json TEXT, -- Données tronquées/masquées (secrets filtrés)
    error_json TEXT
);
```

### 6.2 Protocole IPC Local (Named Pipe Windows / Unix Socket)
* **Endpoint :** `\\.\pipe\wfl-daemon` (Windows) ou `/run/user/$UID/wfl-daemon.sock` (Linux).
* **Messages `ClientRequest` (`wf-ui` / `wf-mcp` → `wf-daemon`) :**
  * `TriggerRun { workflow_name, inputs, dry_run }`
  * `CancelRun { run_id }`
  * `ReloadWorkflows`
  * `SubscribeLiveEvents` (flux temps réel pour colorer et animer les nœuds dans AORUI)
* **Messages `DaemonEvent` (`wf-daemon` → `wf-ui`) :**
  * `RunStarted { run_id, workflow_name, dry_run }`
  * `StepStateChanged { run_id, step_id, status, duration_ms, preview_data }`
  * `RunFinished { run_id, status }`

---

## 7. Plan d'Implémentation Séquentiel pour l'Agent

1. **Étape 1 (`wf-core` — AST & Validation) :**
   * Initialiser le workspace Cargo.
   * Implémenter `ast.rs`, le générateur de schéma JSON (`schemars`) et le validateur sémantique (`validator.rs` : vérification des `step.id`, `connections`, et graphe de variables MiniJinja/Rhai).
   * Écrire les tests unitaires de rejet sur YAML malformés ou références cassées.
2. **Étape 2 (`wf-core` — Runtime & Connecteurs) :**
   * Implémenter `Context` (résolution `env` / `keyring` avec masquage automatique des secrets dans les logs).
   * Implémenter l'adaptateur `OpenDAL` pour `vfs.*` et `lettre` pour `mail.send`.
   * Implémenter l'exécuteur récursif `Engine::execute_steps` avec gestion `timeout_ms`, `retry`, `on_error` et mode `dry_run`.
3. **Étape 3 (`wf-daemon` & SQLite WAL) :**
   * Implémenter la couche de persistance SQLite WAL, le scheduler `cron`, les watchers (`vfs_watch`) et le serveur IPC.
4. **Étape 4 (`wf-mcp` & `wf-ai`) :**
   * Implémenter le serveur MCP pour **Earnest Operations Center** (`wfl_get_grammar`, `wfl_validate`, `wfl_save_draft`, `wfl_run`, `wfl_inspect_run`).
   * Implémenter la boucle d'auto-réparation YAML (`wf-ai`) connectée à l'API locale OpenAI-compatible (`llama-server` / Ollama).
5. **Étape 5 (`wf-ui` — Intégration AORUI) :**
   * Connecter l'AST `WorkflowSpec` au moteur de rendu graphe SDF d'**AORUI** (double vue YAML ↔ Graphe, inspecteur de `step_logs` depuis SQLite, bouton de scellement SHA-256 pour promouvoir un workflow `draft` en `sealed`).