//! iakatc-core — coeur de mesure du daemon iakaTokenCounter.
//!
//! Logique **pure et testable** (aucune dependance Tauri, aucun acces broker) :
//! - `measure` : comptage de conso par projet x agent (Claude Code + Codex).
//! - `quota`   : lecture des fichiers quota/config, fusion hybride 4 branches -> `Reservoir`.
//! - `aggregate` : re-sommation selon les deux axes du contrat (projet x agent, ia x agent).
//! - `publish` : mapping des mesures -> couples (topic-code, payload scalaire `{v,t}`) du
//!   `contrat-mqtt-conso.md`.
//!
//! Le binaire `iakatc-daemon` orchestre ces briques (tick, MQTT, capture statusline).

pub mod aggregate;
pub mod measure;
pub mod publish;
pub mod quota;

/// Renvoie l'instant courant en epoch **secondes** (UTC). Helper unique pour dater les
/// payloads `{v,t}` du contrat.
pub fn now_epoch_s() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
