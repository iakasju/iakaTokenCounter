//! iakatc-daemon — daemon de mesure headless (D7) + sous-commande `statusline-capture` (D4).
//!
//! Deux modes selon le 1er argument :
//! - `iakatc-daemon statusline-capture` : lit le JSON statusline sur stdin, persiste le quota,
//!   re-emet une ligne minimale (ne casse jamais la statusline).
//! - `iakatc-daemon` (sans argument) : boucle de tick — re-scan complet des logs Claude+Codex,
//!   fusion quota, publication MQTT retained code/value selon `contrat-mqtt-conso.md`.
//!
//! Hors-ligne : broker injoignable -> mesure + journalisation + retente + republication a la
//! reconnexion. Le daemon ne crashe pas.

mod config;
mod mqtt;
mod statusline;

use std::collections::HashMap;

use iakatc_core::measure::{claude, codex, Measurement, Provider};
use iakatc_core::publish::contract;
use iakatc_core::quota::{config as qconfig, merge, resolve_home};
use iakatc_core::now_epoch_s;

use crate::config::DaemonConfig;
use crate::mqtt::MqttPublisher;

/// Version publiee dans `meta/daemon/version` (suit la version du crate).
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("statusline-capture") {
        // La sous-commande ne doit JAMAIS casser la statusline : toujours code 0.
        let cfg = DaemonConfig::from_env();
        statusline::run(&cfg.account_label);
        return;
    }
    run_daemon();
}

/// Boucle de vie du daemon : tick periodique, mesure -> fusion -> publication retained.
fn run_daemon() {
    let cfg = DaemonConfig::from_env();
    eprintln!(
        "[iakatc] daemon v{VERSION} — broker {}:{}, racine '{}', tick {}s",
        cfg.host,
        cfg.port,
        cfg.root,
        cfg.tick.as_secs()
    );
    let publisher = MqttPublisher::connect(&cfg);

    loop {
        tick(&cfg, &publisher);
        std::thread::sleep(cfg.tick);
    }
}

/// Un tick : re-scan des logs (recalcul depuis le disque, pas d'increment memoire), fusion du
/// quota, construction des messages du contrat, publication retained.
fn tick(cfg: &DaemonConfig, publisher: &MqttPublisher) {
    let now = now_epoch_s();

    // --- Mesure conso (Claude + Codex) ---
    let mut measurements: Vec<Measurement> = Vec::new();
    if let Some(dir) = claude::claude_projects_dir() {
        measurements.extend(claude::scan_claude_measurements(&dir));
    }
    let mut codex_rl = Vec::new();
    if let Some(dir) = codex::codex_sessions_dir() {
        let (m, rl) = codex::scan_codex(&dir);
        measurements.extend(m);
        codex_rl = rl;
    }

    // --- Fusion quota (Claude statusline + estimation) ---
    let quota_cfg = resolve_home()
        .map(|home| qconfig::load_config(&home))
        .unwrap_or_default();
    let quota_files = resolve_home()
        .map(|home| iakatc_core::quota::store::load_quota_files(&home))
        .unwrap_or_default();

    let mut measured_by_provider: HashMap<String, u64> = HashMap::new();
    for provider in [Provider::Claude, Provider::Codex] {
        let used = iakatc_core::aggregate::used_tokens_by_provider(&measurements, provider);
        if used > 0 {
            measured_by_provider.insert(provider.code().to_string(), used);
        }
    }

    let mut reservoirs = merge::merge(&quota_files, &quota_cfg, &measured_by_provider, now);
    // Quota Codex best-effort (D3) : n'ajoute rien tant que la fenetre ne mappe pas 5h/7d.
    let codex_used = measured_by_provider.get("codex").copied();
    reservoirs.extend(merge::codex_reservoirs(
        &cfg.account_label,
        &codex_rl,
        codex_used,
        now,
    ));

    // --- Construction + publication des messages du contrat ---
    let broker_connected = publisher.is_connected();
    let messages = contract::tick_messages(
        &cfg.root,
        &measurements,
        &reservoirs,
        &quota_cfg,
        "up",
        now,
        broker_connected,
        VERSION,
        now,
    );
    for m in &messages {
        publisher.publish(&m.topic, &m.payload);
    }
    eprintln!(
        "[iakatc] tick {} — {} codes publies (broker {})",
        now,
        messages.len(),
        if broker_connected { "connecte" } else { "hors-ligne" }
    );
}
