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
    Local {
        root: String,
    },
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
    Dropbox {
        access_token: String,
    },
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
    Password {
        password: String,
    },
    KeyFile {
        path: String,
        #[serde(default)]
        passphrase: Option<String>,
    },
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
pub enum ParamType {
    String,
    Int,
    Float,
    Bool,
    List,
    Object,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
pub struct VfsLocation {
    pub conn: String,
    pub path: String,
}

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
pub enum ReadEncoding {
    #[default]
    Utf8,
    Base64,
}

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
pub struct FlowSetParams {
    pub var: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowSleepParams {
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowCallParams {
    pub workflow_path: String,
    #[serde(default)]
    pub inputs: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowFailParams {
    pub code: String,
    pub message: String,
}

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
pub enum LogLevel {
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

fn default_timezone() -> String {
    "UTC".to_string()
}
fn default_poll_ms() -> u64 {
    30_000
}
fn default_inbox() -> String {
    "INBOX".to_string()
}
fn default_drive() -> String {
    "Documents".to_string()
}
fn default_sftp_port() -> u16 {
    22
}
fn default_ftp_port() -> u16 {
    21
}
fn default_smtp_port() -> u16 {
    587
}
fn default_imap_port() -> u16 {
    993
}
fn default_backoff_ms() -> u64 {
    500
}
fn default_concurrency() -> usize {
    1
}
fn default_fetch_limit() -> usize {
    20
}
fn default_true() -> bool {
    true
}
