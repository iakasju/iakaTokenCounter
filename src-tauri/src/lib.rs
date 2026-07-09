//! GUI tray iakaTokenCounter (Tauri v2). Pur **subscriber** du contrat MQTT retained : le backend
//! Rust tient l'etat, la webview rend. Assemble : tray (D4), subscriber (D2), spawn du backbone
//! `iakahub` en sidecar (D1 ; iakahub porte le broker MQTT local et le measure daemon voisin),
//! hook analytics (D6), degradation hors-ligne (D5).

mod analytics;
mod config;
mod history;
pub mod icon;
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
            state::open_analytics,
            history::get_history
        ])
        .setup(move |app| {
            // macOS : app tray-only. Politique d'activation `Accessory` (equiv. LSUIElement) =>
            // pas d'icone au Dock ni d'entree dans le selecteur d'apps (Cmd-Tab). Le tray et la
            // popover restent pleinement fonctionnels.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            tray::build_tray(&handle)?;

            // D1/iakahub : spawn du backbone iakahub en sidecar (il porte le broker MQTT local
            // ET spawne le measure daemon a cote de lui), sauf IAKATC_SPAWN_DAEMON=false. Echec =
            // pas de crash, juste `daemon_available=false` (banniere cote webview).
            let available = spawn_backbone(&handle, &cfg);
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
            // iakahub (limite assumee) : le backbone sidecar s'arrete avec la GUI ; iakahub
            // termine alors le measure daemon a cote de lui (arret en cascade, pas d'orphelin).
            if let RunEvent::Exit = event {
                if let Some(child) = app.state::<AppState>().daemon_child.lock().unwrap().take() {
                    let _ = child.kill();
                }
            }
        });
}

/// Spawne le backbone `iakahub` en sidecar (broker MQTT local + orchestration du measure daemon
/// voisin). Retourne `true` si le backbone est repute disponible (spawn OK, ou spawn desactive
/// volontairement => suppose un iakahub gere par le systeme).
fn spawn_backbone(app: &tauri::AppHandle, cfg: &config::TrayConfig) -> bool {
    if !cfg.spawn_daemon {
        // Choix explicite : un iakahub est gere ailleurs -> subscriber pur, pas de banniere.
        return true;
    }
    match app.shell().sidecar("iakahub") {
        Ok(cmd) => match cmd.spawn() {
            Ok((mut rx, child)) => {
                *app.state::<AppState>().daemon_child.lock().unwrap() = Some(child);
                // Draine les evenements du sidecar pour ne pas bloquer ses pipes.
                tauri::async_runtime::spawn(async move {
                    use tauri_plugin_shell::process::CommandEvent;
                    while let Some(ev) = rx.recv().await {
                        if let CommandEvent::Stderr(line) = ev {
                            eprintln!("[iakahub] {}", String::from_utf8_lossy(&line));
                        }
                    }
                });
                true
            }
            Err(e) => {
                eprintln!("[iakatc-tray] spawn d'iakahub echoue: {e} — mode subscriber pur.");
                false
            }
        },
        Err(e) => {
            eprintln!("[iakatc-tray] sidecar iakahub introuvable: {e} — mode subscriber pur.");
            false
        }
    }
}
