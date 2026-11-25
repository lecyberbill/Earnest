use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread::ThreadId;
use std::time::{Duration, Instant};
use serde_json::Value;
use shell_guard::{allow_for_session, inspect_command_in_workspace, AllowScope, Verdict};

static LIVE_STREAMS: OnceLock<Mutex<HashMap<ThreadId, mpsc::Sender<Vec<u8>>>>> = OnceLock::new();
static PENDING_DECISIONS: OnceLock<Mutex<HashMap<String, mpsc::Sender<bool>>>> = OnceLock::new();

pub struct LiveApprovalStreamGuard {
    tid: ThreadId,
}

impl Drop for LiveApprovalStreamGuard {
    fn drop(&mut self) {
        if let Some(lock) = LIVE_STREAMS.get() {
            if let Ok(mut map) = lock.lock() {
                map.remove(&self.tid);
            }
        }
    }
}

pub fn attach_live_approval_stream(tx: mpsc::Sender<Vec<u8>>) -> LiveApprovalStreamGuard {
    let tid = std::thread::current().id();
    let lock = LIVE_STREAMS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut map) = lock.lock() {
        map.insert(tid, tx);
    }
    LiveApprovalStreamGuard { tid }
}

pub fn resolve_pending_approval(approval_id: &str, approved: bool) -> bool {
    let lock = PENDING_DECISIONS.get_or_init(|| Mutex::new(HashMap::new()));
    let sender = if let Ok(mut map) = lock.lock() {
        if approval_id.is_empty() {
            let first_key = map.keys().next().cloned();
            first_key.and_then(|k| map.remove(&k))
        } else {
            map.remove(approval_id)
        }
    } else {
        None
    };
    if let Some(tx) = sender {
        let _ = tx.send(approved);
        true
    } else {
        false
    }
}

fn ask_live_stream_approval(
    full_command: &str,
    cmd_name: &str,
    kind: &str,
    reason: &str,
    suggestion: &str,
    workspace: &PathBuf,
) -> Option<bool> {
    let tid = std::thread::current().id();
    let sse_tx = LIVE_STREAMS
        .get()
        .and_then(|l| l.lock().ok())
        .and_then(|m| m.get(&tid).cloned())?;

    let approval_id = format!("sg_{}", &uuid::Uuid::new_v4().to_string().replace('-', "")[..10]);
    let (dec_tx, dec_rx) = mpsc::channel::<bool>();
    {
        let lock = PENDING_DECISIONS.get_or_init(|| Mutex::new(HashMap::new()));
        if let Ok(mut map) = lock.lock() {
            map.insert(approval_id.clone(), dec_tx);
        }
    }

    let event_payload = serde_json::json!({
        "approval_id": approval_id,
        "kind": kind,
        "command": full_command,
        "command_name": cmd_name,
        "reason": reason,
        "suggestion": suggestion,
        "workspace": workspace.display().to_string()
    });
    if sse_tx
        .send(format!("event: approval_request\ndata: {}\n\n", event_payload).into_bytes())
        .is_err()
    {
        if let Some(lock) = PENDING_DECISIONS.get() {
            if let Ok(mut map) = lock.lock() {
                map.remove(&approval_id);
            }
        }
        return None;
    }

    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline {
        if crate::llm::client::is_cancelled() {
            break;
        }
        match dec_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(decision) => return Some(decision),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    if let Some(lock) = PENDING_DECISIONS.get() {
        if let Ok(mut map) = lock.lock() {
            map.remove(&approval_id);
        }
    }
    Some(false)
}

pub fn is_forbidden(command: &str) -> bool {
    matches!(
        inspect_command_in_workspace(command, None),
        Verdict::DangerousBlocked { .. }
    )
}

pub fn execute_terminal(command: &str, timeout_secs: u64) -> Result<Value, String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    match inspect_command_in_workspace(command, Some(&cwd)) {
        Verdict::DangerousBlocked {
            command: cmd_name,
            reason,
        } => {
            if let Some(approved) =
                ask_live_stream_approval(command, &cmd_name, "dangerous", &reason, "", &cwd)
            {
                if approved {
                    allow_for_session(&cwd, command, &cmd_name, AllowScope::ExactCommand);
                } else {
                    return Ok(serde_json::json!({
                        "status": "blocked_by_user",
                        "exit_code": -1,
                        "stdout": "",
                        "stderr": format!(
                            "Commande destructrice refusée par l'utilisateur ('{}') : {}",
                            cmd_name, reason
                        ),
                        "duration_ms": 0
                    }));
                }
            } else {
                return Err(format!(
                    "Commande destructrice bloquée par shell-guard ('{}') : {}",
                    cmd_name, reason
                ));
            }
        }
        Verdict::InteractiveBlocked {
            command: cmd_name,
            reason,
            suggestion,
        } => {
            if let Some(approved) = ask_live_stream_approval(
                command,
                &cmd_name,
                "interactive",
                &reason,
                &suggestion,
                &cwd,
            ) {
                if approved {
                    allow_for_session(&cwd, command, &cmd_name, AllowScope::ProgramName);
                } else {
                    return Ok(serde_json::json!({
                        "status": "blocked_interactive",
                        "exit_code": -1,
                        "stdout": "",
                        "stderr": format!(
                            "Blocage maintenu par l'utilisateur ('{}') : {} Suggestion non-interactive : {}",
                            cmd_name, reason, suggestion
                        ),
                        "duration_ms": 0
                    }));
                }
            } else {
                return Ok(serde_json::json!({
                    "status": "blocked_interactive",
                    "exit_code": -1,
                    "stdout": "",
                    "stderr": format!(
                        "Commande interactive bloquée ('{}') : {} Suggestion non-interactive : {}",
                        cmd_name, reason, suggestion
                    ),
                    "duration_ms": 0
                }));
            }
        }
        Verdict::Allowed { .. } => {}
    }

    #[cfg(target_os = "windows")]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", command]);
        c.creation_flags(CREATE_NO_WINDOW);
        c
    };

    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };

    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let start = Instant::now();
    let mut child = cmd.spawn().map_err(|e| format!("Échec du lancement du sous-processus : {}", e))?;

    let mut child_out = child.stdout.take();
    let mut child_err = child.stderr.take();

    let out_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut r) = child_out {
            let _ = r.read_to_end(&mut buf);
        }
        buf
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut r) = child_err {
            let _ = r.read_to_end(&mut buf);
        }
        buf
    });

    let effective_secs = if timeout_secs == 0 { 60 } else { timeout_secs.clamp(1, 600) };
    let limit = Duration::from_secs(effective_secs);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if start.elapsed() >= limit {
                    timed_out = true;
                    #[cfg(target_os = "windows")]
                    {
                        use std::os::windows::process::CommandExt;
                        let pid = child.id().to_string();
                        let _ = Command::new("taskkill")
                            .args(["/T", "/F", "/PID", &pid])
                            .creation_flags(0x0800_0000)
                            .output();
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("Erreur suivi sous-processus : {}", e)),
        }
    };

    let duration_ms = start.elapsed().as_millis();
    let stdout = String::from_utf8_lossy(&out_handle.join().unwrap_or_default()).to_string();
    let stderr = String::from_utf8_lossy(&err_handle.join().unwrap_or_default()).to_string();

    if timed_out {
        return Ok(serde_json::json!({
            "status": "timeout",
            "exit_code": -1,
            "stdout": stdout,
            "stderr": format!("Commande interrompue après {}s (timeout). {}", effective_secs, stderr),
            "duration_ms": duration_ms
        }));
    }

    let exit_code = status.and_then(|s| s.code()).unwrap_or(-1);
    Ok(serde_json::json!({
        "status": if exit_code == 0 { "success" } else { "error" },
        "exit_code": exit_code,
        "stdout": stdout,
        "stderr": stderr,
        "duration_ms": duration_ms
    }))
}

#[cfg(test)]
mod tests {
    use super::execute_terminal;

    #[test]
    fn execute_terminal_runs_quick_command() {
        let res = execute_terminal("echo ok_term", 10).unwrap();
        assert_eq!(res["status"], "success");
        assert!(res["stdout"].as_str().unwrap_or("").contains("ok_term"));
    }

    #[test]
    fn execute_terminal_kills_command_on_timeout() {
        #[cfg(target_os = "windows")]
        let cmd = "Start-Sleep -Seconds 5";
        #[cfg(not(target_os = "windows"))]
        let cmd = "sleep 5";

        let res = execute_terminal(cmd, 1).unwrap();
        assert_eq!(res["status"], "timeout");
    }

    #[test]
    fn execute_terminal_blocks_interactive_and_dangerous_commands_instantly() {
        let inter = execute_terminal("git commit", 5).unwrap();
        assert_eq!(inter["status"], "blocked_interactive");
        assert!(inter["stderr"].as_str().unwrap_or("").contains("-m"));

        let dang = execute_terminal("rm -rf /", 5);
        assert!(dang.is_err());
    }
}
