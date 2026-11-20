pub mod db;
pub mod llm;
pub mod tools;
pub mod engine;
pub mod server;
pub mod lsp;
pub mod mcp;
pub mod governance;

/// Crée un `std::process::Command` silencieux (sans fenêtre console sous Windows).
pub fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}



