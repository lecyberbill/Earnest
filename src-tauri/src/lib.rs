use std::thread;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  // Démarrer le serveur HTTP Earnest Core en tâche de fond sur le port 8080 (si non déjà en écoute)
  thread::spawn(|| {
    earnest_core::server::start_http_server("127.0.0.1", 8080);
  });

  tauri::Builder::default()
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      Ok(())
    })
    .build(tauri::generate_context!())
    .expect("error while building tauri application")
    .run(|_app_handle, event| {
      if matches!(event, tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }) {
        #[cfg(windows)]
        {
          use std::os::windows::process::CommandExt;
          let _ = std::process::Command::new("taskkill")
            .args(["/F", "/IM", "whisper-server.exe"])
            .creation_flags(0x08000000)
            .output();
        }
        #[cfg(not(windows))]
        {
          let _ = std::process::Command::new("pkill")
            .args(["-f", "whisper-server"])
            .output();
        }
      }
    });
}
