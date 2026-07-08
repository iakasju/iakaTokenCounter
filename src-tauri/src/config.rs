//! config — coordonnees broker + options d'execution de la GUI, lues depuis l'environnement.
//!
//! La GUI et le daemon lisent **les memes variables** (contrat § 6) pour pointer le meme broker
//! (defaut : iakahub local `127.0.0.1`). `IAKATC_SPAWN_DAEMON` (defaut `true`) : mettre `false`
//! pour ne pas spawner le backbone `iakahub` en sidecar (cas d'un iakahub deja gere par le systeme).

/// Racine de topic par defaut (contrat § 1).
pub const DEFAULT_ROOT: &str = "iakatokencounter";
/// Broker par defaut : **iakahub local** (`127.0.0.1`), le backbone standalone du poste.
/// Surchargeable par `IAKATC_MQTT_HOST` (ex. pour pointer un broker distant).
pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 1883;

/// Configuration d'execution de la GUI tray.
#[derive(Debug, Clone)]
pub struct TrayConfig {
    pub host: String,
    pub port: u16,
    pub user: Option<String>,
    pub password: Option<String>,
    pub root: String,
    pub client_id: String,
    /// Spawner le backbone `iakahub` en sidecar au demarrage. `false` => subscriber pur.
    pub spawn_daemon: bool,
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

impl TrayConfig {
    /// Construit la config depuis les variables d'env (contrat § 6 + `IAKATC_SPAWN_DAEMON`).
    pub fn from_env() -> Self {
        let host = env("IAKATC_MQTT_HOST").unwrap_or_else(|| DEFAULT_HOST.to_string());
        let port = env("IAKATC_MQTT_PORT")
            .and_then(|p| p.parse().ok())
            .unwrap_or(DEFAULT_PORT);
        let user = env("IAKATC_MQTT_USER").or_else(|| env("MOSQUITTO_USER"));
        let password = env("IAKATC_MQTT_PASSWORD").or_else(|| env("MOSQUITTO_PASSWORD"));
        let root = env("IAKATC_MQTT_ROOT").unwrap_or_else(|| DEFAULT_ROOT.to_string());
        let client_id = env("IAKATC_MQTT_CLIENT_ID")
            .unwrap_or_else(|| format!("iakatc-tray-{}", hostname()));
        // Spawn par defaut ; seule la valeur explicite "false" desactive.
        let spawn_daemon = env("IAKATC_SPAWN_DAEMON")
            .map(|v| !v.eq_ignore_ascii_case("false"))
            .unwrap_or(true);
        TrayConfig {
            host,
            port,
            user,
            password,
            root,
            client_id,
            spawn_daemon,
        }
    }
}

fn hostname() -> String {
    env("HOSTNAME")
        .or_else(|| env("COMPUTERNAME"))
        .unwrap_or_else(|| "host".to_string())
}
