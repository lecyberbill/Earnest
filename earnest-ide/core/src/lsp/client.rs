use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, serde::Serialize)]
pub struct LspCatalogEntry {
    pub language: &'static str,
    pub display_name: &'static str,
    pub binary: &'static str,
    pub args: &'static [&'static str],
    pub install_cmd: &'static str,
}

pub const LSP_CATALOG: &[LspCatalogEntry] = &[
    LspCatalogEntry {
        language: "rust",
        display_name: "Rust (rust-analyzer)",
        binary: "rust-analyzer",
        args: &[],
        install_cmd: "rustup component add rust-analyzer",
    },
    LspCatalogEntry {
        language: "python",
        display_name: "Python (pylsp)",
        binary: "pylsp",
        args: &[],
        install_cmd: "pip install python-lsp-server",
    },
    LspCatalogEntry {
        language: "typescript",
        display_name: "TypeScript / JS",
        binary: "typescript-language-server",
        args: &["--stdio"],
        install_cmd: "npm install -g typescript-language-server typescript",
    },
    LspCatalogEntry {
        language: "javascript",
        display_name: "JavaScript (tsserver)",
        binary: "typescript-language-server",
        args: &["--stdio"],
        install_cmd: "npm install -g typescript-language-server typescript",
    },
    LspCatalogEntry {
        language: "go",
        display_name: "Go (gopls)",
        binary: "gopls",
        args: &["serve"],
        install_cmd: "go install golang.org/x/tools/gopls@latest",
    },
    LspCatalogEntry {
        language: "c",
        display_name: "C (clangd)",
        binary: "clangd",
        args: &[],
        install_cmd: "winget install LLVM.LLVM",
    },
    LspCatalogEntry {
        language: "cpp",
        display_name: "C++ (clangd)",
        binary: "clangd",
        args: &[],
        install_cmd: "winget install LLVM.LLVM",
    },
    LspCatalogEntry {
        language: "toml",
        display_name: "TOML (taplo)",
        binary: "taplo",
        args: &["lsp", "stdio"],
        install_cmd: "cargo install taplo-cli --locked",
    },
    LspCatalogEntry {
        language: "yaml",
        display_name: "YAML (yaml-language-server)",
        binary: "yaml-language-server",
        args: &["--stdio"],
        install_cmd: "npm install -g yaml-language-server",
    },
    LspCatalogEntry {
        language: "json",
        display_name: "JSON (vscode-json-language-server)",
        binary: "vscode-json-language-server",
        args: &["--stdio"],
        install_cmd: "npm install -g vscode-langservers-extracted",
    },
    LspCatalogEntry {
        language: "html",
        display_name: "HTML (vscode-html-language-server)",
        binary: "vscode-html-language-server",
        args: &["--stdio"],
        install_cmd: "npm install -g vscode-langservers-extracted",
    },
    LspCatalogEntry {
        language: "css",
        display_name: "CSS (vscode-css-language-server)",
        binary: "vscode-css-language-server",
        args: &["--stdio"],
        install_cmd: "npm install -g vscode-langservers-extracted",
    },
];

pub fn catalog_for(lang: &str) -> Option<&'static LspCatalogEntry> {
    let norm = lang.to_lowercase();
    LSP_CATALOG.iter().find(|e| e.language == norm)
}

pub fn find_binary_in_path(bin: &str) -> Option<PathBuf> {
    let candidates: Vec<String> = if cfg!(windows) {
        vec![
            format!("{}.exe", bin),
            format!("{}.cmd", bin),
            bin.to_string(),
        ]
    } else {
        vec![bin.to_string()]
    };

    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            for cand in &candidates {
                let p = dir.join(cand);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

pub fn path_to_uri(path: &str) -> String {
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));
    let mut s = abs.to_string_lossy().replace('\\', "/");
    if s.starts_with("//?/") {
        s = s.trim_start_matches("//?/").to_string();
    }
    if !s.starts_with('/') {
        format!("file:///{}", s)
    } else {
        format!("file://{}", s)
    }
}

struct LspProcess {
    stdin: Mutex<ChildStdin>,
    pending: Mutex<HashMap<i64, mpsc::Sender<Value>>>,
    diagnostics: Mutex<HashMap<String, Vec<Value>>>,
    opened_docs: Mutex<HashMap<String, i64>>,
    next_id: AtomicI64,
    _child: Mutex<Child>,
}

impl LspProcess {
    fn start_arc(binary: &Path, args: &[&str], root_path: &str) -> Result<std::sync::Arc<Self>, String> {
        let mut cmd = crate::hidden_command(binary);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if Path::new(root_path).is_dir() {
            cmd.current_dir(root_path);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Impossible de démarrer {}: {}", binary.display(), e))?;
        let stdin = child.stdin.take().ok_or("stdin LSP indisponible")?;
        let stdout = child.stdout.take().ok_or("stdout LSP indisponible")?;

        let proc_arc = std::sync::Arc::new(Self {
            stdin: Mutex::new(stdin),
            pending: Mutex::new(HashMap::new()),
            diagnostics: Mutex::new(HashMap::new()),
            opened_docs: Mutex::new(HashMap::new()),
            next_id: AtomicI64::new(1),
            _child: Mutex::new(child),
        });

        let reader_proc = std::sync::Arc::downgrade(&proc_arc);
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut content_len: Option<usize> = None;
                let mut header_line = String::new();
                loop {
                    header_line.clear();
                    match reader.read_line(&mut header_line) {
                        Ok(0) | Err(_) => return,
                        Ok(_) => {
                            let trimmed = header_line.trim();
                            if trimmed.is_empty() {
                                break;
                            }
                            if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
                                content_len = rest.trim().parse::<usize>().ok();
                            }
                        }
                    }
                }
                let Some(len) = content_len else {
                    continue;
                };
                let mut buf = vec![0u8; len];
                if reader.read_exact(&mut buf).is_err() {
                    return;
                }
                let Ok(msg) = serde_json::from_slice::<Value>(&buf) else {
                    continue;
                };
                let Some(p) = reader_proc.upgrade() else {
                    return;
                };
                if let Some(id) = msg.get("id").and_then(|v| v.as_i64()) {
                    if let Ok(mut map) = p.pending.lock() {
                        if let Some(tx) = map.remove(&id) {
                            let _ = tx.send(msg);
                        }
                    }
                } else if msg.get("method").and_then(|v| v.as_str()) == Some("textDocument/publishDiagnostics") {
                    if let Some(uri) = msg["params"]["uri"].as_str() {
                        if let Some(diags) = msg["params"]["diagnostics"].as_array() {
                            if let Ok(mut dmap) = p.diagnostics.lock() {
                                dmap.insert(uri.to_string(), diags.clone());
                            }
                        }
                    }
                }
            }
        });

        let root_uri = path_to_uri(root_path);
        let init_params = json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "completion": { "completionItem": { "snippetSupport": false } },
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "publishDiagnostics": {}
                }
            }
        });
        let _ = proc_arc.request_raw("initialize", init_params, Duration::from_secs(6))?;
        let _ = proc_arc.notify_raw("initialized", json!({}));
        Ok(proc_arc)
    }

    fn write_frame(&self, body: &Value) -> Result<(), String> {
        let payload = serde_json::to_string(body).map_err(|e| e.to_string())?;
        let frame = format!("Content-Length: {}\r\n\r\n{}", payload.len(), payload);
        let mut stdin = self.stdin.lock().map_err(|_| "stdin poisoned".to_string())?;
        stdin
            .write_all(frame.as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|e| format!("Erreur écriture LSP: {}", e))
    }

    fn notify_raw(&self, method: &str, params: Value) -> Result<(), String> {
        self.write_frame(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }))
    }

    fn request_raw(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        {
            let mut map = self.pending.lock().map_err(|_| "pending poisoned".to_string())?;
            map.insert(id, tx);
        }
        if let Err(e) = self.write_frame(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        })) {
            if let Ok(mut map) = self.pending.lock() {
                map.remove(&id);
            }
            return Err(e);
        }
        match rx.recv_timeout(timeout) {
            Ok(msg) => Ok(msg),
            Err(_) => {
                if let Ok(mut map) = self.pending.lock() {
                    map.remove(&id);
                }
                Err(format!("Timeout LSP sur {}", method))
            }
        }
    }

    fn sync_document(&self, uri: &str, lang: &str, text: &str) {
        let version = {
            let mut docs = match self.opened_docs.lock() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(v) = docs.get_mut(uri) {
                *v += 1;
                Some(*v)
            } else {
                docs.insert(uri.to_string(), 1);
                None
            }
        };
        if let Some(ver) = version {
            let _ = self.notify_raw(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri, "version": ver },
                    "contentChanges": [{ "text": text }]
                }),
            );
        } else {
            let _ = self.notify_raw(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": lang,
                        "version": 1,
                        "text": text
                    }
                }),
            );
        }
    }
}

pub struct LspManager {
    clients: Mutex<HashMap<(String, String), std::sync::Arc<LspProcess>>>,
}

static LSP_MANAGER: OnceLock<LspManager> = OnceLock::new();

impl LspManager {
    pub fn global() -> &'static Self {
        LSP_MANAGER.get_or_init(|| Self {
            clients: Mutex::new(HashMap::new()),
        })
    }

    fn get_or_start(&self, lang: &str, root: &str) -> Result<std::sync::Arc<LspProcess>, String> {
        let entry = catalog_for(lang).ok_or_else(|| format!("Langage {} sans entrée LSP", lang))?;
        let key = (entry.binary.to_string(), root.to_string());
        {
            let guard = self.clients.lock().map_err(|_| "LSP mutex poisoned".to_string())?;
            if let Some(c) = guard.get(&key) {
                return Ok(c.clone());
            }
        }
        let bin = find_binary_in_path(entry.binary)
            .ok_or_else(|| format!("{} introuvable dans PATH", entry.binary))?;
        let proc_arc = LspProcess::start_arc(&bin, entry.args, root)?;
        let mut guard = self.clients.lock().map_err(|_| "LSP mutex poisoned".to_string())?;
        guard.insert(key, proc_arc.clone());
        Ok(proc_arc)
    }

    pub fn complete(&self, lang: &str, root: &str, path: &str, code: &str, line: u64, col: u64) -> Option<Vec<Value>> {
        let client = self.get_or_start(lang, root).ok()?;
        let uri = path_to_uri(path);
        client.sync_document(&uri, lang, code);
        let resp = client
            .request_raw(
                "textDocument/completion",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line.saturating_sub(1), "character": col.saturating_sub(1) }
                }),
                Duration::from_secs(2),
            )
            .ok()?;
        let result = &resp["result"];
        let arr = result["items"].as_array().or_else(|| result.as_array())?;
        if arr.is_empty() {
            return None;
        }
        let mapped = arr
            .iter()
            .take(50)
            .map(|it| {
                json!({
                    "label": it["label"].as_str().unwrap_or(""),
                    "kind": it["kind"].as_u64().unwrap_or(1),
                    "detail": it["detail"].as_str().unwrap_or("lsp"),
                    "documentation": it["documentation"]["value"].as_str().or_else(|| it["documentation"].as_str()).unwrap_or("")
                })
            })
            .collect();
        Some(mapped)
    }

    pub fn hover(&self, lang: &str, root: &str, path: &str, code: &str, line: u64, col: u64) -> Option<Vec<String>> {
        let client = self.get_or_start(lang, root).ok()?;
        let uri = path_to_uri(path);
        client.sync_document(&uri, lang, code);
        let resp = client
            .request_raw(
                "textDocument/hover",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line.saturating_sub(1), "character": col.saturating_sub(1) }
                }),
                Duration::from_secs(2),
            )
            .ok()?;
        let contents = &resp["result"]["contents"];
        let mut out = Vec::new();
        if let Some(s) = contents.as_str() {
            if !s.trim().is_empty() {
                out.push(s.to_string());
            }
        } else if let Some(s) = contents["value"].as_str() {
            if !s.trim().is_empty() {
                out.push(s.to_string());
            }
        } else if let Some(arr) = contents.as_array() {
            for item in arr {
                if let Some(s) = item.as_str().or_else(|| item["value"].as_str()) {
                    if !s.trim().is_empty() {
                        out.push(s.to_string());
                    }
                }
            }
        }
        if out.is_empty() { None } else { Some(out) }
    }

    pub fn definition(&self, lang: &str, root: &str, path: &str, code: &str, line: u64, col: u64) -> Option<Vec<Value>> {
        let client = self.get_or_start(lang, root).ok()?;
        let uri = path_to_uri(path);
        client.sync_document(&uri, lang, code);
        let resp = client
            .request_raw(
                "textDocument/definition",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line.saturating_sub(1), "character": col.saturating_sub(1) }
                }),
                Duration::from_secs(2),
            )
            .ok()?;
        let res = &resp["result"];
        let loc = if let Some(arr) = res.as_array() {
            arr.first()?.clone()
        } else if res.is_object() {
            res.clone()
        } else {
            return None;
        };
        let target_uri = loc["uri"].as_str().or_else(|| loc["targetUri"].as_str())?;
        let target_path = target_uri
            .strip_prefix("file:///")
            .or_else(|| target_uri.strip_prefix("file://"))
            .unwrap_or(target_uri)
            .to_string();
        let start = loc["range"]["start"].as_object().or_else(|| loc["targetSelectionRange"]["start"].as_object())?;
        let ln = start.get("line").and_then(|v| v.as_u64()).unwrap_or(0) + 1;
        let ch = start.get("character").and_then(|v| v.as_u64()).unwrap_or(0) + 1;
        Some(vec![json!({
            "file": target_path,
            "line": ln,
            "column": ch
        })])
    }

    pub fn diagnostics_for(&self, lang: &str, root: &str, path: &str, code: &str) -> Vec<Value> {
        let Ok(client) = self.get_or_start(lang, root) else {
            return Vec::new();
        };
        let uri = path_to_uri(path);
        client.sync_document(&uri, lang, code);
        let Ok(map) = client.diagnostics.lock() else {
            return Vec::new();
        };
        let Some(diags) = map.get(&uri) else {
            return Vec::new();
        };
        diags
            .iter()
            .filter_map(|d| {
                let msg = d["message"].as_str()?;
                let r = &d["range"];
                let start_line = r["start"]["line"].as_u64().unwrap_or(0) + 1;
                let start_col = r["start"]["character"].as_u64().unwrap_or(0) + 1;
                let end_line = r["end"]["line"].as_u64().unwrap_or(start_line - 1) + 1;
                let end_col = r["end"]["character"].as_u64().unwrap_or(start_col) + 1;
                let severity = d["severity"].as_u64().unwrap_or(1);
                Some(json!({
                    "start_line": start_line,
                    "start_col": start_col,
                    "end_line": end_line,
                    "end_col": end_col,
                    "severity": severity,
                    "message": msg
                }))
            })
            .collect()
    }

    pub fn status_list(&self) -> Vec<Value> {
        let guard = self.clients.lock().ok();
        LSP_CATALOG
            .iter()
            .map(|e| {
                let installed = find_binary_in_path(e.binary).is_some();
                let running = guard
                    .as_ref()
                    .map(|g| g.keys().any(|(b, _)| b == e.binary))
                    .unwrap_or(false);
                json!({
                    "language": e.language,
                    "display_name": e.display_name,
                    "binary": e.binary,
                    "installed": installed,
                    "running": running,
                    "install_cmd": e.install_cmd
                })
            })
            .collect()
    }

    pub fn install_language(&self, lang: &str) -> Result<String, String> {
        let entry = catalog_for(lang)
            .ok_or_else(|| format!("Aucun serveur LSP connu pour '{}'", lang))?;
        let mut cmd = if cfg!(windows) {
            let mut c = crate::hidden_command("powershell");
            c.args(["-NoProfile", "-NonInteractive", "-Command", entry.install_cmd]);
            c
        } else {
            let mut c = crate::hidden_command("sh");
            c.args(["-c", entry.install_cmd]);
            c
        };
        let out = cmd
            .output()
            .map_err(|e| format!("Échec lancement installation {}: {}", entry.binary, e))?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if out.status.success() {
            Ok(format!("{} installé avec succès.\n{}", entry.binary, stdout.trim()))
        } else {
            Err(format!("Échec installation {} (code {:?}):\n{}\n{}", entry.binary, out.status.code(), stdout.trim(), stderr.trim()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_resolves_known_languages() {
        assert_eq!(catalog_for("rust").unwrap().binary, "rust-analyzer");
        assert_eq!(catalog_for("go").unwrap().binary, "gopls");
        assert!(catalog_for("unknown_xyz").is_none());
    }
}
