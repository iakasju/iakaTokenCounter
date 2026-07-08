//! iakahub — binaire d'orchestration de vie du backbone local (D1..D5).
//!
//! Sequence : init tracing -> **broker** local (thread, fail-fast si port occupe) -> attente
//! d'ecoute -> localisation + **spawn/supervision** du daemon (env broker injecte) -> attente d'un
//! **signal d'arret** -> **arret en cascade** (kill du daemon) -> sortie. Toute erreur de demarrage
//! = **log clair + code de sortie != 0** (jamais de crash silencieux).

use std::process::Child;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::Duration;

use iakahub::{broker, shutdown, supervisor};
use supervisor::RestartPolicy;

/// Intervalle de scrutation du superviseur (assez court pour un arret reactif).
const POLL_INTERVAL: Duration = Duration::from_millis(200);

fn main() {
    init_tracing();

    let port = broker::port_from_env();

    // (1) Broker local. Echec (port occupe / non demarre) = fail-fast, code != 0 (D5).
    let addr = match broker::start(port) {
        Ok(addr) => addr,
        Err(e) => {
            tracing::error!(error = %e, "demarrage du broker impossible");
            std::process::exit(2);
        }
    };
    tracing::info!(%addr, "iakahub: backbone local pret");

    // (2) Drapeau + handler d'arret (Ctrl-C / SIGTERM). Sans lui, on ne peut pas garantir la
    //     cascade -> echec explicite.
    let stop = shutdown::new_flag();
    if let Err(e) = shutdown::install(stop.clone()) {
        tracing::error!(error = %e, "handler d'arret non installe");
        std::process::exit(3);
    }

    // (3) Localisation + supervision du daemon (a cote de l'executable). Absent -> le broker
    //     reste actif, on journalise et on attend l'arret (D5).
    match supervisor::resolve_daemon_path() {
        Some(path) => {
            tracing::info!(daemon = %path.display(), "daemon localise — spawn + supervision");
            let policy = RestartPolicy::default();
            let slot: Mutex<Option<Child>> = Mutex::new(None);
            // Bloque jusqu'a arret demande OU abandon apres redemarrages bornes.
            supervisor::supervise(
                || supervisor::spawn_daemon(&path, port),
                &policy,
                &stop,
                &slot,
                POLL_INTERVAL,
            );
        }
        None => {
            tracing::warn!(
                "iatc-daemon introuvable a cote de l'executable — broker seul (jauges vides)"
            );
        }
    }

    // (4) Si on est sorti sur abandon (daemon KO) et non sur signal, on maintient le broker
    //     vivant jusqu'au signal d'arret (D5 : broker reste actif).
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(POLL_INTERVAL);
    }

    tracing::info!("iakahub: arret propre (daemon termine, aucun orphelin)");
}

/// Initialise `tracing` (niveau via `RUST_LOG`, defaut `info`). N'echoue jamais durement.
fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}
