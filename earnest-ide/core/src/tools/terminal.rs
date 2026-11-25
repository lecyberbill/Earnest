use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use serde_json::Value;
use shell_guard::{inspect_command_in_workspace, Verdict};

pub fn is_forbidden(command: &str) -> bool {
    matches!(
        inspect_command_in_workspace(command, None),
        Verdict::DangerousBlocked { .. }
    )
}

pub fn execute_terminal(command: &str, timeout_secs: u64) -> Result<Value, String> {
    execute_terminal_in_dir(command, timeout_secs, None)
}

pub fn execute_terminal_with_approval(
    command: &str,
    timeout_secs: u64,
    cwd: Option<&Path>,
    request_ui_approval: bool,
) -> (Result<Value, String>, Option<String>) {
    let effective_dir: PathBuf = cwd
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    match inspect_command_in_workspace(command, Some(&effective_dir)) {
        Verdict::DangerousBlocked {
            command: cmd_name,
            reason,
        } => {
            if request_ui_approval {
                let payload = serde_json::json!({
                    "status": "awaiting_approval",
                    "action": "shell_guard_blocked",
                    "kind": "dangerous",
                    "full_command": command,
                    "command_name": cmd_name,
                    "reason": reason,
                    "suggestion": "",
                    "workspace": effective_dir.display().to_string(),
                    "message": format!("Commande critique bloquée ('{}'). En attente de décision utilisateur pour ce dossier/session...", cmd_name)
                });
                return (Ok(payload), Some("AwaitingShellGuardApproval".to_string()));
            }
            return (
                Err(format!(
                    "Commande destructrice bloquée par shell-guard ('{}') : {}",
                    cmd_name, reason
                )),
                None,
            );
        }
        Verdict::InteractiveBlocked {
            command: cmd_name,
            reason,
            suggestion,
        } => {
            if request_ui_approval {
                let payload = serde_json::json!({
                    "status": "awaiting_approval",
                    "action": "shell_guard_blocked",
                    "kind": "interactive",
                    "full_command": command,
                    "command_name": cmd_name,
                    "reason": reason,
                    "suggestion": suggestion,
                    "workspace": effective_dir.display().to_string(),
                    "message": format!("Commande interactive bloquée ('{}'). En attente de déblocage utilisateur ou d'alternative...", cmd_name)
                });
                return (Ok(payload), Some("AwaitingShellGuardApproval".to_string()));
            }
            return (
                Ok(serde_json::json!({
                    "status": "blocked_interactive",
                    "exit_code": -1,
                    "stdout": "",
                    "stderr": format!(
                        "Commande interactive bloquée ('{}') : {} Suggestion non-interactive : {}",
                        cmd_name, reason, suggestion
                    ),
                    "duration_ms": 0
                })),
                None,
            );
        }
        Verdict::Allowed { .. } => {}
    }

    (run_subprocess(command, timeout_secs, Some(&effective_dir)), None)
}

pub fn execute_terminal_in_dir(
    command: &str,
    timeout_secs: u64,
    cwd: Option<&Path>,
) -> Result<Value, String> {
    execute_terminal_with_approval(command, timeout_secs, cwd, false).0
}

fn run_subprocess(command: &str, timeout_secs: u64, cwd: Option<&Path>) -> Result<Value, String> {
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = crate::hidden_command("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", command]);
        c
    };

    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = crate::hidden_command("sh");
        c.args(["-c", command]);
        c
    };

    if let Some(dir) = cwd {
        if dir.is_dir() {
            cmd.current_dir(dir);
        }
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let start = Instant::now();
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Échec du lancement du sous-processus : {}", e))?;

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
                        let pid = child.id().to_string();
                        let _ = crate::hidden_command("taskkill")
                            .args(["/T", "/F", "/PID", &pid])
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
