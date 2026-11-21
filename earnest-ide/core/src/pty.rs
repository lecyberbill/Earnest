use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;

const MAX_HISTORY_BYTES: usize = 64 * 1024;

struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    history: Vec<u8>,
    subscribers: Vec<mpsc::Sender<Vec<u8>>>,
}

pub struct PtyManager {
    sessions: Mutex<HashMap<String, PtySession>>,
}

static PTY_MANAGER: OnceLock<PtyManager> = OnceLock::new();

impl PtyManager {
    pub fn global() -> &'static Self {
        PTY_MANAGER.get_or_init(|| Self {
            sessions: Mutex::new(HashMap::new()),
        })
    }

    pub fn ensure_session(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        {
            let guard = self.sessions.lock().map_err(|_| "PTY mutex poisoned".to_string())?;
            if guard.contains_key(id) {
                return Ok(());
            }
        }

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: rows.max(8),
                cols: cols.max(20),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Impossible d'ouvrir le PTY: {}", e))?;

        let mut cmd = if cfg!(target_os = "windows") {
            let mut c = CommandBuilder::new("powershell.exe");
            c.arg("-NoLogo");
            c
        } else {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
            CommandBuilder::new(shell)
        };
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("Impossible de lancer le shell PTY: {}", e))?;
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Impossible de cloner le lecteur PTY: {}", e))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("Impossible d'ouvrir le writer PTY: {}", e))?;

        {
            let mut guard = self.sessions.lock().map_err(|_| "PTY mutex poisoned".to_string())?;
            if guard.contains_key(id) {
                return Ok(());
            }
            guard.insert(
                id.to_string(),
                PtySession {
                    master: pair.master,
                    writer,
                    history: Vec::new(),
                    subscribers: Vec::new(),
                },
            );
        }

        let session_id = id.to_string();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = buf[..n].to_vec();
                        if let Ok(mut guard) = PtyManager::global().sessions.lock() {
                            if let Some(sess) = guard.get_mut(&session_id) {
                                sess.history.extend_from_slice(&chunk);
                                if sess.history.len() > MAX_HISTORY_BYTES {
                                    let excess = sess.history.len() - MAX_HISTORY_BYTES;
                                    sess.history.drain(..excess);
                                }
                                sess.subscribers.retain(|tx| tx.send(chunk.clone()).is_ok());
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
            let _ = child.wait();
            if let Ok(mut guard) = PtyManager::global().sessions.lock() {
                guard.remove(&session_id);
            }
        });

        Ok(())
    }

    pub fn subscribe(
        &self,
        id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(Vec<u8>, mpsc::Receiver<Vec<u8>>), String> {
        self.ensure_session(id, cols, rows)?;
        let (tx, rx) = mpsc::channel();
        let mut guard = self.sessions.lock().map_err(|_| "PTY mutex poisoned".to_string())?;
        let sess = guard
            .get_mut(id)
            .ok_or_else(|| "Session PTY introuvable".to_string())?;
        let snapshot = sess.history.clone();
        sess.subscribers.push(tx);
        Ok((snapshot, rx))
    }

    pub fn write(&self, id: &str, data: &[u8]) -> Result<(), String> {
        let mut guard = self.sessions.lock().map_err(|_| "PTY mutex poisoned".to_string())?;
        let sess = guard
            .get_mut(id)
            .ok_or_else(|| "Session PTY introuvable".to_string())?;
        sess.writer
            .write_all(data)
            .and_then(|_| sess.writer.flush())
            .map_err(|e| format!("Erreur écriture PTY: {}", e))
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let guard = self.sessions.lock().map_err(|_| "PTY mutex poisoned".to_string())?;
        let sess = guard
            .get(id)
            .ok_or_else(|| "Session PTY introuvable".to_string())?;
        sess.master
            .resize(PtySize {
                rows: rows.max(4),
                cols: cols.max(10),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Erreur resize PTY: {}", e))
    }

    pub fn restart(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        {
            let mut guard = self.sessions.lock().map_err(|_| "PTY mutex poisoned".to_string())?;
            guard.remove(id);
        }
        self.ensure_session(id, cols, rows)
    }
}

pub fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { TABLE[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[(n & 63) as usize] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn pty_spawns_shell_and_echoes_output() {
        let mgr = PtyManager::global();
        let (snap, rx) = mgr.subscribe("test-pty-unit", 80, 24).expect("subscribe failed");
        mgr.write("test-pty-unit", b"echo PTY_UNIT_OK\r").expect("write failed");
        let mut collected = String::from_utf8_lossy(&snap).into_owned();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && !collected.contains("PTY_UNIT_OK") {
            if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(250)) {
                collected.push_str(&String::from_utf8_lossy(&chunk));
            }
        }
        assert!(
            collected.contains("PTY_UNIT_OK"),
            "Expected PTY_UNIT_OK in output, got: {:?}",
            collected
        );
    }
}
