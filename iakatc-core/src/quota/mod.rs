//! quota — capture, configuration et fusion hybride du quota (le « Reservoir »).
//!
//! - `store`  : resolution de `IAKATC_HOME`, lecture des fichiers `quota/<provider>.<account>.json`
//!   ecrits par la sous-commande `statusline-capture` (D4).
//! - `config` : lecture de `IAKATC_HOME/config.json` (seuils de fraicheur + plafonds, tout
//!   optionnel) (D5).
//! - `merge`  : fusion **hybride** en 4 branches (official / official_stale / local_estimate /
//!   none) -> un [`Reservoir`] par `(account, provider, window)` (D5).

pub mod config;
pub mod merge;
pub mod store;

use std::path::{Path, PathBuf};

/// Sous-dossier du HOME utilise par defaut si `IAKATC_HOME` n'est pas pose.
const DEFAULT_HOME_SUBDIR: &str = ".iakatokencounter";

/// Resout `IAKATC_HOME` : la variable d'env si posee, sinon `<home>/.iakatokencounter`.
/// `None` si aucun home determinable (rare ; le daemon degrade proprement).
pub fn resolve_home() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("IAKATC_HOME") {
        return Some(PathBuf::from(h));
    }
    dirs::home_dir().map(|h| h.join(DEFAULT_HOME_SUBDIR))
}

/// Dossier des fichiers quota : `<IAKATC_HOME>/quota`.
pub fn quota_dir(home: &Path) -> PathBuf {
    home.join("quota")
}
