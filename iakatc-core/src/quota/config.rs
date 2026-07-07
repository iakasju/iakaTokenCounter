//! quota::config — lecture de `IAKATC_HOME/config.json` (D5).
//!
//! Tout est **optionnel** : fichier absent ou malforme -> valeurs par defaut (+ warning),
//! jamais de crash. Plafonds Pro/Max non publies par Anthropic -> `null` par defaut (choix MVP :
//! ne pas inventer de plafond faux). Tant qu'un plafond est `null`, l'estimation `used_pct` reste
//! `null`.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Seuil de fraicheur par defaut de la fenetre 5h (secondes) : 20 min.
pub const DEFAULT_FRESH_5H_SECONDS: i64 = 1200;
/// Seuil de fraicheur par defaut de la fenetre 7d (secondes) : 6 h.
pub const DEFAULT_FRESH_7D_SECONDS: i64 = 21600;

/// Seuils de fraicheur (au-dela desquels un quota exact devient `official_stale`).
#[derive(Debug, Clone, Deserialize)]
pub struct Freshness {
    #[serde(default = "default_fresh_5h")]
    pub five_hour_seconds: i64,
    #[serde(default = "default_fresh_7d")]
    pub seven_day_seconds: i64,
}

fn default_fresh_5h() -> i64 {
    DEFAULT_FRESH_5H_SECONDS
}
fn default_fresh_7d() -> i64 {
    DEFAULT_FRESH_7D_SECONDS
}

impl Default for Freshness {
    fn default() -> Self {
        Freshness {
            five_hour_seconds: DEFAULT_FRESH_5H_SECONDS,
            seven_day_seconds: DEFAULT_FRESH_7D_SECONDS,
        }
    }
}

/// Plafonds de tokens configures pour un compte (fenetre 5h et 7d). `None` = non configure.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Ceiling {
    #[serde(default)]
    pub five_hour_tokens: Option<u64>,
    #[serde(default)]
    pub seven_day_tokens: Option<u64>,
}

/// Configuration complete (fraicheur + plafonds par provider/compte).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub freshness: Freshness,
    /// `ceilings[provider][account]` -> plafonds. Ex. `ceilings["claude"]["max"]`.
    #[serde(default)]
    pub ceilings: HashMap<String, HashMap<String, Ceiling>>,
}

impl Config {
    /// Plafond configure pour `(provider, account)`, ou `Ceiling` vide (deux `None`) sinon.
    pub fn ceiling(&self, provider: &str, account: &str) -> Ceiling {
        self.ceilings
            .get(provider)
            .and_then(|by_acc| by_acc.get(account))
            .cloned()
            .unwrap_or_default()
    }
}

/// Charge `<home>/config.json`. Absent ou malforme -> `Config::default()` (+ warning sur stderr).
pub fn load_config(home: &Path) -> Config {
    let path = home.join("config.json");
    let raw = match std::fs::read_to_string(&path) {
        Ok(r) => r,
        Err(_) => return Config::default(), // absence normale : pas de warning.
    };
    match serde_json::from_str::<Config>(&raw) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[iakatc] config.json ignore (malforme) : {} ({e})",
                path.display()
            );
            Config::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_par_defaut_seuils_standards() {
        let c = Config::default();
        assert_eq!(c.freshness.five_hour_seconds, 1200);
        assert_eq!(c.freshness.seven_day_seconds, 21600);
        assert!(c.ceilings.is_empty());
    }

    #[test]
    fn deserialise_config_partielle_complete_les_defauts() {
        // Seule la fenetre 5h de fraicheur est fournie -> 7d prend le defaut.
        let c: Config =
            serde_json::from_str(r#"{"freshness":{"five_hour_seconds":600}}"#).unwrap();
        assert_eq!(c.freshness.five_hour_seconds, 600);
        assert_eq!(c.freshness.seven_day_seconds, 21600);
    }

    #[test]
    fn ceiling_lu_ou_vide() {
        let c: Config = serde_json::from_str(
            r#"{"ceilings":{"claude":{"max":{"five_hour_tokens":1000000,"seven_day_tokens":null}}}}"#,
        )
        .unwrap();
        let ceil = c.ceiling("claude", "max");
        assert_eq!(ceil.five_hour_tokens, Some(1_000_000));
        assert_eq!(ceil.seven_day_tokens, None);
        // Compte inconnu -> plafonds None.
        assert_eq!(c.ceiling("claude", "inconnu").five_hour_tokens, None);
    }

    #[test]
    fn load_config_absent_rend_defaut() {
        let dir = std::env::temp_dir().join(format!("iakatc-cfg-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let c = load_config(&dir);
        assert_eq!(c.freshness.five_hour_seconds, 1200);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
