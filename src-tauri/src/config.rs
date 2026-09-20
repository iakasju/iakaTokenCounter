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

/// Seuil de liveness par defaut (s) des agents « en cours » (feature-agents-en-cours.md, D2) : un
/// agent est tournant ssi `now - mtime(transcript) <= N`. Arbitrage decideur entre clignotement
/// (trop court) et fantomes (trop long) — cf. instruction. Surchargeable par `IAKATC_LIVENESS_SECS`
/// pour affiner en recette sans rebuild.
pub const DEFAULT_LIVENESS_SECS: u64 = 90;

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

/// Seuil de liveness (s) des agents en cours, lu depuis `IAKATC_LIVENESS_SECS` (meme patron que
/// les autres variables ci-dessus). Valeur absente/invalide -> [`DEFAULT_LIVENESS_SECS`].
pub fn liveness_secs_from_env() -> u64 {
    env("IAKATC_LIVENESS_SECS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_LIVENESS_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liveness_defaut_a_90_sans_variable() {
        // On ne touche pas reellement a l'environnement du process de test (partage entre tests
        // paralleles) : on verifie seulement la valeur de repli directement.
        assert_eq!(DEFAULT_LIVENESS_SECS, 90);
    }
}
