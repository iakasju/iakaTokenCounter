//! shutdown — arret propre / signal (D3).
//!
//! Installe un handler Ctrl-C / SIGTERM (et, sous Windows, l'evenement de fermeture) qui **leve
//! un drapeau d'arret** partage. Le superviseur observe ce drapeau, **tue l'enfant daemon** (arret
//! en cascade, pas d'orphelin) puis rend la main. Le drapeau est aussi lisible par le `main` pour
//! sortir de sa boucle d'attente.
//!
//! Limite MVP assumee : un `SIGKILL` (non capturable) sur iakahub ne declenche pas la cascade
//! logicielle — c'est un invariant OS. Les chemins SIGINT/SIGTERM/fermeture normale, eux, ne
//! laissent aucun daemon survivant.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Drapeau d'arret partage entre le handler de signal, le superviseur et le `main`.
pub type StopFlag = Arc<AtomicBool>;

/// Cree un drapeau d'arret (initialement `false`).
pub fn new_flag() -> StopFlag {
    Arc::new(AtomicBool::new(false))
}

/// Installe le handler de signal : au premier Ctrl-C / SIGTERM, le drapeau passe a `true`.
/// Idempotent cote OS pour les signaux suivants (une seule installation).
pub fn install(stop: StopFlag) -> Result<(), String> {
    ctrlc::set_handler(move || {
        stop.store(true, Ordering::SeqCst);
    })
    .map_err(|e| format!("installation du handler d'arret impossible: {e}"))
}

/// Vrai si un arret a ete demande.
pub fn is_stopping(stop: &StopFlag) -> bool {
    stop.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drapeau_neuf_est_non_arrete() {
        let flag = new_flag();
        assert!(!is_stopping(&flag));
    }

    #[test]
    fn lever_le_drapeau_est_visible() {
        let flag = new_flag();
        flag.store(true, Ordering::SeqCst);
        assert!(is_stopping(&flag));
    }
}
