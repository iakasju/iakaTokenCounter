//! config — lecture de la configuration d'execution du daemon depuis l'environnement
//! (coordonnees broker § 6 du contrat, cadence de tick, etiquette de compte). Aucun secret n'est
//! commite : identifiants via env uniquement.

use std::time::Duration;

/// Racine de topic par defaut (contrat § 1).
pub const DEFAULT_ROOT: &str = "iakatokencounter";
/// Broker Mosquitto iakabox par defaut (contrat § 6).
pub const DEFAULT_HOST: &str = "192.168.2.11";
pub const DEFAULT_PORT: u16 = 1883;
/// Cadence de tick par defaut (D7).
pub const DEFAULT_TICK_SECONDS: u64 = 60;

/// Configuration d'execution resolue depuis l'environnement.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub host: String,
    pub port: u16,
    pub user: Option<String>,
    pub password: Option<String>,
    pub root: String,
    pub client_id: String,
    pub tick: Duration,
    /// Etiquette de compte (multi-comptes ; la statusline n'a pas d'ID de compte).
    pub account_label: String,
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

impl DaemonConfig {
    /// Construit la config depuis les variables d'env (contrat § 6), avec repli sur les defauts.
    pub fn from_env() -> Self {
        let host = env("IAKATC_MQTT_HOST").unwrap_or_else(|| DEFAULT_HOST.to_string());
        let port = env("IAKATC_MQTT_PORT")
            .and_then(|p| p.parse().ok())
            .unwrap_or(DEFAULT_PORT);
        let user = env("IAKATC_MQTT_USER").or_else(|| env("MOSQUITTO_USER"));
        let password = env("IAKATC_MQTT_PASSWORD").or_else(|| env("MOSQUITTO_PASSWORD"));
        let root = env("IAKATC_MQTT_ROOT").unwrap_or_else(|| DEFAULT_ROOT.to_string());
        let host_suffix = hostname();
        let client_id =
            env("IAKATC_MQTT_CLIENT_ID").unwrap_or_else(|| format!("iakatc-daemon-{host_suffix}"));
        let tick = env("IAKATC_TICK_SECONDS")
            .and_then(|s| s.parse().ok())
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(DEFAULT_TICK_SECONDS));
        let account_label = env("IAKATC_ACCOUNT_LABEL").unwrap_or_else(|| "default".to_string());
        DaemonConfig {
            host,
            port,
            user,
            password,
            root,
            client_id,
            tick,
            account_label,
        }
    }
}

/// Nom d'hote (best-effort) pour composer un client id distinct par poste.
fn hostname() -> String {
    env("HOSTNAME")
        .or_else(|| env("COMPUTERNAME"))
        .unwrap_or_else(|| "host".to_string())
}
