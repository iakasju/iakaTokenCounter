//! mqtt_sub — subscriber rumqttc (thread dedie). S'abonne en retained aux topics de quota du
//! contrat, met a jour l'etat en memoire et pousse l'instantane a la webview (`tray://state`)
//! + rafraichit le tooltip du tray.
//!
//! Hors-ligne / broker down (D5) : le thread ne panique jamais ; `rumqttc` retente en tache de
//! fond (backoff), l'etat passe « broker deconnecte », et le **retained** repeuple les jauges a la
//! reconnexion (les abonnements sont (re)poses sur chaque `ConnAck`). QoS 1 (contrat § 4).

use rumqttc::{Client, Event, MqttOptions, Packet, QoS};
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::config::TrayConfig;
use crate::state::AppState;

/// Nom d'evenement pousse a la webview a chaque changement d'etat.
pub const STATE_EVENT: &str = "tray://state";

/// Lance le subscriber dans un thread dedie. Ne bloque pas ; ne panique jamais sur le reseau.
pub fn start(app: AppHandle, cfg: TrayConfig) {
    std::thread::spawn(move || run(app, cfg));
}

fn run(app: AppHandle, cfg: TrayConfig) {
    let mut opts = MqttOptions::new(cfg.client_id.clone(), cfg.host.clone(), cfg.port);
    opts.set_keep_alive(Duration::from_secs(30));
    if let (Some(u), Some(p)) = (&cfg.user, &cfg.password) {
        opts.set_credentials(u.clone(), p.clone());
    }
    let (client, mut connection) = Client::new(opts, 128);

    let quota_sub = format!("{}/all/ia/+/+/quota/#", cfg.root);
    let meta_sub = format!("{}/meta/daemon/#", cfg.root);

    // `connection.iter()` boucle indefiniment et gere la reconnexion : on ne sort jamais.
    for notification in connection.iter() {
        match notification {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                set_connected(&app, true);
                // (Re)abonnements a chaque connexion : le retained repeuple immediatement.
                let _ = client.subscribe(&quota_sub, QoS::AtLeastOnce);
                let _ = client.subscribe(&meta_sub, QoS::AtLeastOnce);
                push_state(&app);
            }
            Ok(Event::Incoming(Packet::Publish(p))) => {
                let changed = handle_publish(&app, &cfg.root, &p.topic, &p.payload);
                if changed {
                    push_state(&app);
                }
            }
            Ok(_) => {}
            Err(e) => {
                set_connected(&app, false);
                push_state(&app);
                eprintln!("[iakatc-tray] MQTT hors-ligne ({e}) — nouvelle tentative…");
                std::thread::sleep(Duration::from_secs(3));
            }
        }
    }
}

/// Traite un Publish : quota -> store ; meta/daemon/state -> disponibilite daemon.
/// Retourne `true` si l'etat rendu a change.
fn handle_publish(app: &AppHandle, root: &str, topic: &str, payload: &[u8]) -> bool {
    let state = app.state::<AppState>();
    // Un daemon (spawne ici ou ailleurs) qui publie sa meta prouve sa disponibilite (D5).
    if topic == format!("{root}/meta/daemon/state/current") {
        let was = state.daemon_available.swap(true, Ordering::Relaxed);
        return !was;
    }
    let mut store = state.store.lock().unwrap();
    store.apply_message(root, topic, payload)
}

fn set_connected(app: &AppHandle, connected: bool) {
    app.state::<AppState>()
        .broker_connected
        .store(connected, Ordering::Relaxed);
}

/// Emet l'instantane vers la webview et met a jour le tooltip du tray.
pub fn push_state(app: &AppHandle) {
    let snapshot = app.state::<AppState>().snapshot();
    crate::tray::update_tooltip(app, snapshot.worst.as_ref(), snapshot.broker_connected);
    // Recompose l'icone (logo + reservoirs du pire compte) a chaque maj d'etat (D2/D3).
    crate::tray::update_icon(app, &snapshot.reservoirs);
    let _ = app.emit(STATE_EVENT, &snapshot);
}
