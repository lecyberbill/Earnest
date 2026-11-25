use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use serde_json::Value;
use shell_guard::{inspect_command, Verdict};

pub fn is_forbidden(command: &str) -> bool {
    matches!(inspect_command(command), Verdict::DangerousBlocked { .. })
}

pub fn execute_terminal(command: &str, timeout_secs: u64) -> Result<Value, String> {
    match inspect_command(command) {
        Verdict::DangerousBlocked { command: cmd_name, reason } => {
            return Err(format!(
                "Commande destructrice bloquée par shell-guard ('{}') : {}",
                cmd_name, reason
            ));
        }
        Verdict::InteractiveBlocked {
            command: cmd_name,
            reason,
            suggestion,
        } => {
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
