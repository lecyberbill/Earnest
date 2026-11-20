use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use serde_json::Value;
use wf_mcp::WfMcpServer;

fn main() {
    let mut workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut db_path: Option<PathBuf> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--workspace" => {
                if let Some(v) = args.next() {
                    workspace = PathBuf::from(v);
                }
            }
            "--db" => {
                if let Some(v) = args.next() {
                    db_path = Some(PathBuf::from(v));
                }
            }
            _ => {}
        }
    }

    let db = db_path.unwrap_or_else(|| workspace.join(".wf").join("wf.db"));
    let server = WfMcpServer::open(&workspace, &db).expect("Impossible d'ouvrir WfStore");

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(req) = serde_json::from_str::<Value>(trimmed) {
            if let Some(resp) = server.handle(&req) {
                let _ = writeln!(stdout, "{}", resp);
                let _ = stdout.flush();
            }
        }
    }
}
