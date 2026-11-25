//! Session-only, workspace-scoped command allowlist.
//!
//! Overrides live strictly in process memory (RAM) and are partitioned by the
//! canonicalized path of the active workspace directory:
//! - Closing the application automatically clears all overrides.
//! - Unblocking a command in folder A never unblocks it in folder B.
//! - `DangerousBlocked` commands can only ever be unlocked by exact string match,
//!   never by broad program name.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::{inspect_command, RiskLevel, Verdict};

/// Granularity of a temporary session unlock within a single workspace folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowScope {
    /// Unlock only this exact command line string in this workspace.
    ExactCommand,
    /// Unlock this exact command line AND this interactive program name in this workspace.
    ProgramName,
}

#[derive(Default)]
struct WorkspaceAllowSet {
    exact_commands: HashSet<String>,
    interactive_programs: HashSet<String>,
}

static SESSION_ALLOWLIST: OnceLock<Mutex<HashMap<String, WorkspaceAllowSet>>> = OnceLock::new();

fn store() -> &'static Mutex<HashMap<String, WorkspaceAllowSet>> {
    SESSION_ALLOWLIST.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Normalizes a workspace directory path into a stable per-folder key.
pub fn normalize_workspace_key(workspace: &Path) -> String {
    let resolved = std::fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let mut s = resolved.to_string_lossy().replace('\\', "/");
    if let Some(stripped) = s.strip_prefix("//?/") {
        s = stripped.to_string();
    }
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    if cfg!(windows) {
        s.to_ascii_lowercase()
    } else {
        s
    }
}

/// Temporarily unlocks a command for the current process session and the given `workspace` folder.
pub fn allow_for_session(
    workspace: &Path,
    full_command: &str,
    program_name: &str,
    scope: AllowScope,
) {
    let key = normalize_workspace_key(workspace);
    let trimmed_cmd = full_command.trim();
    if let Ok(mut guard) = store().lock() {
        let entry = guard.entry(key).or_default();
        if !trimmed_cmd.is_empty() {
            entry.exact_commands.insert(trimmed_cmd.to_string());
        }
        if scope == AllowScope::ProgramName {
            let norm_prog = program_name.trim().to_ascii_lowercase();
            if !norm_prog.is_empty() {
                entry.interactive_programs.insert(norm_prog);
            }
        }
    }
}

/// Inspects `cmd`, honoring any in-memory session overrides registered for `workspace`.
pub fn inspect_command_in_workspace(cmd: &str, workspace: Option<&Path>) -> Verdict {
    let verdict = inspect_command(cmd);
    let Some(ws) = workspace else {
        return verdict;
    };
    if verdict.is_allowed() {
        return verdict;
    }

    let key = normalize_workspace_key(ws);
    let trimmed = cmd.trim();
    if let Ok(guard) = store().lock() {
        if let Some(allow_set) = guard.get(&key) {
            match &verdict {
                Verdict::DangerousBlocked { .. } => {
                    if allow_set.exact_commands.contains(trimmed) {
                        return Verdict::Allowed {
                            risk: RiskLevel::Mutating,
                        };
                    }
                }
                Verdict::InteractiveBlocked { command, .. } => {
                    if allow_set.exact_commands.contains(trimmed)
                        || allow_set
                            .interactive_programs
                            .contains(&command.to_ascii_lowercase())
                    {
                        return Verdict::Allowed {
                            risk: RiskLevel::Mutating,
                        };
                    }
                }
                Verdict::Allowed { .. } => {}
            }
        }
    }

    verdict
}

/// Clears session overrides for a specific `workspace` (or all workspaces when `None`).
pub fn clear_session_overrides(workspace: Option<&Path>) {
    if let Ok(mut guard) = store().lock() {
        match workspace {
            Some(ws) => {
                let key = normalize_workspace_key(ws);
                guard.remove(&key);
            }
            None => guard.clear(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_session_allowlist_is_strictly_scoped_per_folder_and_danger_exactness() {
        let dir_a = PathBuf::from("test_ws_alpha_unique_991");
        let dir_b = PathBuf::from("test_ws_beta_unique_992");
        clear_session_overrides(Some(&dir_a));
        clear_session_overrides(Some(&dir_b));

        // Initially blocked in both folders
        assert!(inspect_command_in_workspace("npm init", Some(&dir_a)).is_blocked());
        assert!(inspect_command_in_workspace("npm init", Some(&dir_b)).is_blocked());

        // Unlock interactive program in dir_a -> allowed in dir_a, STILL blocked in dir_b
        allow_for_session(&dir_a, "npm init", "npm init", AllowScope::ProgramName);
        assert!(inspect_command_in_workspace("npm init", Some(&dir_a)).is_allowed());
        assert!(inspect_command_in_workspace("npm init", Some(&dir_b)).is_blocked());

        // Dangerous command: ProgramName scope does NOT unlock a different dangerous command
        allow_for_session(&dir_a, "shutdown -h now", "shutdown", AllowScope::ProgramName);
        assert!(inspect_command_in_workspace("shutdown -h now", Some(&dir_a)).is_allowed());
        assert!(inspect_command_in_workspace("shutdown -r now", Some(&dir_a)).is_blocked());
        assert!(inspect_command_in_workspace("shutdown -h now", Some(&dir_b)).is_blocked());

        clear_session_overrides(Some(&dir_a));
        assert!(inspect_command_in_workspace("npm init", Some(&dir_a)).is_blocked());
    }
}
