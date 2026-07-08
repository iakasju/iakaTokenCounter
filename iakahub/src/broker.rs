//! broker — broker MQTT `rumqttd` in-process, local et anonyme (D1).
//!
//! Construit une [`rumqttd::Config`] a partir d'un gabarit TOML embarque (`include_str!`) dont
//! seul le champ `listen` est substitue (`127.0.0.1:<port>`), puis lance `Broker::start()` — qui
//! est **bloquant** — dans un **thread dedie**. Un unique listener **MQTT v4** ; **aucune auth**.
//!
//! Robustesse au demarrage (D5) : le port est **pre-teste** (`TcpListener::bind`) pour un
//! **fail-fast** deterministe si `127.0.0.1:<port>` est deja pris ; puis on **sonde** en connexion
//! TCP que le broker ecoute avant de rendre la main (pas d'auto-increment de port).
//!
//! Note d'implementation verifiee sur `rumqttd` 0.19 : si le bind d'un listener echoue, le thread
//! serveur interne **journalise et se termine** (l'erreur ne remonte pas via `start()`). Le
//! pre-test du port est donc la garantie de fail-fast ; la sonde couvre le cas « broker non parti ».

use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use rumqttd::{Broker, Config};

/// Adresse de bind du broker : boucle locale uniquement (jamais expose au reseau, D1).
pub const BIND_IP: Ipv4Addr = Ipv4Addr::LOCALHOST;
/// Port MQTT par defaut (contrat § 6, surchargeable via `IAKATC_MQTT_PORT`).
pub const DEFAULT_PORT: u16 = 1883;

/// Placeholder du gabarit TOML, substitue par `127.0.0.1:<port>` au demarrage.
const LISTEN_PLACEHOLDER: &str = "__IAKAHUB_LISTEN__";
/// Gabarit de configuration embarque (un seul listener v4, anonyme).
const CONFIG_TEMPLATE: &str = include_str!("../rumqttd.toml");

/// Lit le port cible depuis l'environnement (`IAKATC_MQTT_PORT`), defaut [`DEFAULT_PORT`].
pub fn port_from_env() -> u16 {
    std::env::var("IAKATC_MQTT_PORT")
        .ok()
        .and_then(|p| p.trim().parse().ok())
        .filter(|p| *p != 0)
        .unwrap_or(DEFAULT_PORT)
}

/// Adresse d'ecoute complete (bind local + port).
pub fn listen_addr(port: u16) -> SocketAddr {
    SocketAddr::from((BIND_IP, port))
}

/// Substitue le placeholder de bind dans le gabarit TOML. Pur et testable.
pub fn render_config_toml(port: u16) -> String {
    CONFIG_TEMPLATE.replace(LISTEN_PLACEHOLDER, &listen_addr(port).to_string())
}

/// Construit la `rumqttd::Config` a partir du gabarit substitue. Erreur = TOML invalide.
pub fn build_config(port: u16) -> Result<Config, String> {
    let toml_str = render_config_toml(port);
    toml::from_str::<Config>(&toml_str).map_err(|e| format!("config rumqttd invalide: {e}"))
}

/// Vrai si le port `127.0.0.1:<port>` peut etre lie maintenant (donc libre).
pub fn port_is_free(port: u16) -> bool {
    TcpListener::bind(listen_addr(port)).is_ok()
}

/// Sonde (bornee) que le broker ecoute : reussit un CONNECT TCP avant l'expiration.
fn wait_until_listening(addr: SocketAddr, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Demarre le broker sur `127.0.0.1:<port>` dans un thread dedie et attend qu'il ecoute.
///
/// Etapes (D1/D5) : (1) pre-test du port -> fail-fast si occupe ; (2) build de la `Config` ;
/// (3) `Broker::start()` (bloquant) dans un thread dedie ; (4) sonde d'ecoute bornee.
/// Retourne l'adresse d'ecoute si le broker est joignable, sinon une erreur explicite.
pub fn start(port: u16) -> Result<SocketAddr, String> {
    let addr = listen_addr(port);

    // (1) Fail-fast deterministe : port deja pris => erreur claire, pas d'auto-increment.
    if !port_is_free(port) {
        return Err(format!(
            "port {addr} deja occupe — libere-le ou fixe IAKATC_MQTT_PORT (pas d'auto-increment)"
        ));
    }

    // (2) Config depuis le gabarit embarque (echoue tot si le TOML est casse).
    let config = build_config(port)?;

    // (3) start() est bloquant -> thread dedie. Une erreur de bind d'un listener est
    //     journalisee par rumqttd et termine le thread (elle ne remonte pas ici) : la sonde
    //     ci-dessous fait foi pour declarer le broker « a l'ecoute ».
    std::thread::Builder::new()
        .name("iakahub-broker".to_string())
        .spawn(move || {
            let mut broker = Broker::new(config);
            if let Err(e) = broker.start() {
                tracing::error!(error = ?e, "broker rumqttd arrete sur erreur");
            }
        })
        .map_err(|e| format!("impossible de lancer le thread broker: {e}"))?;

    // (4) Sonde d'ecoute (borne courte) : confirme que le listener est vivant.
    if wait_until_listening(addr, Duration::from_secs(5)) {
        tracing::info!(%addr, "broker MQTT local a l'ecoute (v4, anonyme)");
        Ok(addr)
    } else {
        Err(format!(
            "le broker n'ecoute pas sur {addr} apres le delai — demarrage echoue"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_substitue_le_port_et_le_bind_local() {
        let rendered = render_config_toml(1883);
        assert!(!rendered.contains(LISTEN_PLACEHOLDER), "placeholder non substitue");
        assert!(
            rendered.contains("listen = \"127.0.0.1:1883\""),
            "bind local/port attendu absent:\n{rendered}"
        );
    }

    #[test]
    fn render_prend_le_port_demande() {
        let rendered = render_config_toml(21999);
        assert!(rendered.contains("127.0.0.1:21999"));
    }

    #[test]
    fn build_config_parse_le_listener_v4_sur_le_bon_port() {
        let cfg = build_config(24567).expect("le gabarit doit produire une Config valide");
        let v4 = cfg.v4.expect("un listener v4 doit etre present");
        let server = v4.values().next().expect("au moins un serveur v4");
        assert_eq!(server.listen, listen_addr(24567));
        // Anonyme : aucune section auth dans le gabarit.
        assert!(server.connections.auth.is_none(), "broker doit rester anonyme");
    }

    #[test]
    fn port_from_env_defaut_est_1883() {
        // Sans surcharge d'env, le defaut du contrat s'applique.
        std::env::remove_var("IAKATC_MQTT_PORT");
        assert_eq!(port_from_env(), DEFAULT_PORT);
    }
}
