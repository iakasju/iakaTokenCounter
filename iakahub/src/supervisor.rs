//! supervisor — localise, spawne et supervise `iakatc-daemon` (D2).
//!
//! iakahub est le **parent orchestrateur** : la GUI ne spawne jamais le daemon directement.
//! Le daemon est cherche **a cote de l'executable iakahub** (meme dossier — vrai dans le bundle
//! Tauri comme en dev `target/`). On lui **injecte l'environnement broker** (creds factices : le
//! broker anonyme les ignore -> zero modif du code daemon). Supervision : **redemarrage borne**
//! (<= [`MAX_RESTARTS`]) puis abandon journalise ; le broker, lui, continue de tourner.
//!
//! La logique de redemarrage est isolee ([`RestartPolicy`], [`supervise`]) et testable sans
//! spawner de vrai process, via le trait [`Supervised`].

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Nom de base du binaire daemon (sans extension).
pub const DAEMON_BIN: &str = "iakatc-daemon";
/// Nombre maximal de redemarrages de l'enfant avant abandon (D2).
pub const MAX_RESTARTS: u32 = 3;

/// Nom de fichier du daemon selon l'OS (`.exe` sous Windows).
pub fn daemon_filename() -> String {
    if cfg!(windows) {
        format!("{DAEMON_BIN}.exe")
    } else {
        DAEMON_BIN.to_string()
    }
}

/// Cherche le daemon dans `dir` (chemin candidat s'il existe). Pur et testable.
pub fn resolve_in(dir: &Path) -> Option<PathBuf> {
    let candidate = dir.join(daemon_filename());
    candidate.is_file().then_some(candidate)
}

/// Localise le daemon **a cote de l'executable iakahub** (meme dossier).
pub fn resolve_daemon_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    resolve_in(dir)
}

/// Variables d'environnement injectees au daemon pour le pointer sur le broker local (D2).
/// Les creds sont **factices** : le broker anonyme les ignore (fait verifie).
pub fn daemon_env(port: u16) -> Vec<(&'static str, String)> {
    vec![
        ("IAKATC_MQTT_HOST", "127.0.0.1".to_string()),
        ("IAKATC_MQTT_PORT", port.to_string()),
        ("IAKATC_MQTT_USER", "iakahub".to_string()),
        ("IAKATC_MQTT_PASSWORD", "local".to_string()),
    ]
}

/// Spawne le daemon localise a `path`, avec l'env broker injecte (D2).
pub fn spawn_daemon(path: &Path, port: u16) -> io::Result<Child> {
    let mut cmd = Command::new(path);
    for (k, v) in daemon_env(port) {
        cmd.env(k, v);
    }
    cmd.spawn()
}

/// Politique de redemarrage bornee (D2).
#[derive(Debug, Clone, Copy)]
pub struct RestartPolicy {
    /// Nombre maximal de redemarrages avant abandon.
    pub max_restarts: u32,
    /// Delai entre deux tentatives.
    pub backoff: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        RestartPolicy {
            max_restarts: MAX_RESTARTS,
            backoff: Duration::from_secs(2),
        }
    }
}

impl RestartPolicy {
    /// Faut-il retenter, sachant que `restarts_done` redemarrages ont deja eu lieu ?
    pub fn should_restart(&self, restarts_done: u32) -> bool {
        restarts_done < self.max_restarts
    }
}

/// Un enfant supervisable : interrogeable (a-t-il quitte ?) et tuable. Implemente par
/// [`std::process::Child`] ; un faux enfant sert aux tests de la boucle de supervision.
pub trait Supervised {
    /// `Ok(Some(code))` si l'enfant a quitte, `Ok(None)` s'il tourne encore.
    fn poll_exit(&mut self) -> io::Result<Option<i32>>;
    /// Termine l'enfant (arret en cascade / kill-on-stop).
    fn stop(&mut self) -> io::Result<()>;
}

impl Supervised for Child {
    fn poll_exit(&mut self) -> io::Result<Option<i32>> {
        Ok(self.try_wait()?.map(|s| s.code().unwrap_or(-1)))
    }
    fn stop(&mut self) -> io::Result<()> {
        self.kill()
    }
}

/// Supervise un enfant avec redemarrage borne (D2). Bloque jusqu'a :
/// - un arret demande (`stop` passe a `true`) -> l'enfant courant est **tue** (pas d'orphelin) ;
/// - un abandon (l'enfant meurt et la [`RestartPolicy`] refuse un nouvel essai).
///
/// `slot` porte l'enfant courant : un arret externe le tue via [`Supervised::stop`]. `poll` est
/// l'intervalle de scrutation (court). Retourne quand la supervision se termine.
pub fn supervise<C, S>(
    mut spawn: S,
    policy: &RestartPolicy,
    stop: &AtomicBool,
    slot: &Mutex<Option<C>>,
    poll: Duration,
) where
    C: Supervised,
    S: FnMut() -> io::Result<C>,
{
    let mut restarts: u32 = 0;
    'outer: loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match spawn() {
            Ok(child) => {
                *slot.lock().unwrap() = Some(child);
            }
            Err(e) => {
                tracing::error!(error = %e, "spawn du daemon impossible — abandon");
                break;
            }
        }
        // Attente de sortie (ou d'arret), par scrutation courte.
        loop {
            if stop.load(Ordering::SeqCst) {
                break 'outer;
            }
            let exited = {
                let mut guard = slot.lock().unwrap();
                match guard.as_mut() {
                    Some(child) => child.poll_exit().unwrap_or(Some(-1)),
                    None => Some(0),
                }
            };
            if let Some(code) = exited {
                tracing::warn!(code, "iatc-daemon s'est arrete de facon inattendue");
                break;
            }
            std::thread::sleep(poll);
        }
        if stop.load(Ordering::SeqCst) {
            break;
        }
        // Decision AVANT increment : `restarts` = nombre de redemarrages deja effectues.
        if !policy.should_restart(restarts) {
            tracing::error!(
                restarts,
                max = policy.max_restarts,
                "daemon abandonne apres redemarrages bornes — le broker reste actif"
            );
            break;
        }
        restarts += 1;
        tracing::warn!(attempt = restarts, "redemarrage du daemon…");
        std::thread::sleep(policy.backoff);
    }
    // Arret en cascade (D3) : tue tout enfant survivant avant de rendre la main.
    if let Some(mut child) = slot.lock().unwrap().take() {
        let _ = child.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_env_pointe_le_broker_local_avec_le_port() {
        let env = daemon_env(24680);
        assert!(env.contains(&("IAKATC_MQTT_HOST", "127.0.0.1".to_string())));
        assert!(env.contains(&("IAKATC_MQTT_PORT", "24680".to_string())));
        // Creds factices presents (le broker anonyme les ignore).
        assert!(env.iter().any(|(k, _)| *k == "IAKATC_MQTT_USER"));
        assert!(env.iter().any(|(k, _)| *k == "IAKATC_MQTT_PASSWORD"));
    }

    #[test]
    fn resolve_in_trouve_le_binaire_voisin_et_rien_sinon() {
        let dir = std::env::temp_dir().join(format!("iakahub-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Absent -> None.
        assert!(resolve_in(&dir).is_none());
        // Present -> Some(chemin voisin).
        let bin = dir.join(daemon_filename());
        std::fs::write(&bin, b"stub").unwrap();
        assert_eq!(resolve_in(&dir), Some(bin));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restart_policy_borne_a_max() {
        let p = RestartPolicy {
            max_restarts: 3,
            backoff: Duration::from_millis(0),
        };
        assert!(p.should_restart(0));
        assert!(p.should_restart(1));
        assert!(p.should_restart(2));
        assert!(!p.should_restart(3)); // 4e mort -> abandon
        assert!(!p.should_restart(4));
    }

    /// Faux enfant qui « meurt » immediatement a chaque poll : sert a compter les spawns.
    struct DeadOnPoll;
    impl Supervised for DeadOnPoll {
        fn poll_exit(&mut self) -> io::Result<Option<i32>> {
            Ok(Some(1))
        }
        fn stop(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn supervise_spawn_exactement_max_plus_un_puis_abandonne() {
        use std::sync::atomic::AtomicU32;
        let spawns = AtomicU32::new(0);
        let stop = AtomicBool::new(false);
        let slot: Mutex<Option<DeadOnPoll>> = Mutex::new(None);
        let policy = RestartPolicy {
            max_restarts: 3,
            backoff: Duration::from_millis(0),
        };
        supervise(
            || {
                spawns.fetch_add(1, Ordering::SeqCst);
                Ok(DeadOnPoll)
            },
            &policy,
            &stop,
            &slot,
            Duration::from_millis(0),
        );
        // 1 demarrage initial + 3 redemarrages = 4 spawns, puis abandon.
        assert_eq!(spawns.load(Ordering::SeqCst), 4);
    }

    /// Faux enfant vivant (ne quitte jamais) : sert a tester l'arret en cascade.
    struct AlwaysAlive {
        stopped: std::sync::Arc<AtomicBool>,
    }
    impl Supervised for AlwaysAlive {
        fn poll_exit(&mut self) -> io::Result<Option<i32>> {
            Ok(None)
        }
        fn stop(&mut self) -> io::Result<()> {
            self.stopped.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn supervise_tue_l_enfant_a_l_arret_demande() {
        use std::sync::Arc;
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let slot: Arc<Mutex<Option<AlwaysAlive>>> = Arc::new(Mutex::new(None));

        let handle = {
            let stopped = stopped.clone();
            let stop = stop.clone();
            let slot = slot.clone();
            std::thread::spawn(move || {
                let policy = RestartPolicy::default();
                supervise(
                    || {
                        Ok(AlwaysAlive {
                            stopped: stopped.clone(),
                        })
                    },
                    &policy,
                    &stop,
                    &slot,
                    Duration::from_millis(5),
                );
            })
        };

        // Laisse le temps de spawner l'enfant, puis demande l'arret.
        std::thread::sleep(Duration::from_millis(50));
        stop.store(true, Ordering::SeqCst);
        handle.join().unwrap();
        assert!(stopped.load(Ordering::SeqCst), "l'enfant doit etre tue a l'arret");
    }
}
