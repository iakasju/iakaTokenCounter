//! GUI tray iakaTokenCounter (Tauri v2). Pur **subscriber** du contrat MQTT retained : le backend
//! Rust tient l'etat, la webview rend. Assemble : tray (D4), subscriber (D2), spawn daemon sidecar
//! (D1), hook analytics (D6), degradation hors-ligne (D5).

mod analytics;
mod config;
mod mqtt_sub;
mod state;
mod tray;

use state::AppState;
use std::sync::atomic::Ordering;
use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_shell::ShellExt;

/// Point d'entree de l'app (appele par `main.rs`).
pub fn run() {
    let cfg = config::TrayConfig::from_env();
    let app_state = AppState::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            state::get_reservoirs,
            state::open_analytics
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            tray::build_tray(&handle)?;

            // D1 : spawn du daemon en sidecar, sauf IAKATC_SPAWN_DAEMON=false. Echec = pas de
            // crash, juste `daemon_available=false` (banniere cote webview).
            let available = spawn_daemon(&handle, &cfg);
            handle
                .state::<AppState>()
                .daemon_available
                .store(available, Ordering::Relaxed);

            // D2 : subscriber MQTT dans un thread dedie (jamais de MQTT dans la webview).
            mqtt_sub::start(handle.clone(), cfg.clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // App tray persistante : la popover se **masque** au lieu de quitter.
            if window.label() == "popover" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("erreur au build de l'app Tauri")
        .run(|app, event| {
            // D1 (limite assumee) : le daemon sidecar s'arrete avec la GUI.
            if let RunEvent::Exit = event {
                if let Some(child) = app.state::<AppState>().daemon_child.lock().unwrap().take() {
                    let _ = child.kill();
                }
            }
        });
}

/// Spawne le daemon `iakatc-daemon` en sidecar. Retourne `true` si le daemon est repute
/// disponible (spawn OK, ou spawn desactive volontairement => suppose un daemon externe).
fn spawn_daemon(app: &tauri::AppHandle, cfg: &config::TrayConfig) -> bool {
    if !cfg.spawn_daemon {
        // Choix explicite (D1) : un daemon est gere ailleurs -> subscriber pur, pas de banniere.
        return true;
    }
    match app.shell().sidecar("iakatc-daemon") {
        Ok(cmd) => match cmd.spawn() {
            Ok((mut rx, child)) => {
                *app.state::<AppState>().daemon_child.lock().unwrap() = Some(child);
                // Draine les evenements du sidecar pour ne pas bloquer ses pipes.
                tauri::async_runtime::spawn(async move {
                    use tauri_plugin_shell::process::CommandEvent;
                    while let Some(ev) = rx.recv().await {
                        if let CommandEvent::Stderr(line) = ev {
                            eprintln!("[iakatc-daemon] {}", String::from_utf8_lossy(&line));
                        }
                    }
                });
                true
            }
            Err(e) => {
                eprintln!("[iakatc-tray] spawn du daemon echoue: {e} — mode subscriber pur.");
                false
            }
        },
        Err(e) => {
            eprintln!("[iakatc-tray] sidecar daemon introuvable: {e} — mode subscriber pur.");
            false
        }
    }
}
