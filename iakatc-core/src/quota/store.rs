//! quota::store — contrat de fichier `quota/<provider>.<account>.json` (D4).
//!
//! Ecrit par la sous-commande `statusline-capture` (un fichier par compte, evite les races),
//! relu a chaque tick par le daemon. LECTURE defensive : un fichier malforme est **ignore avec
//! warning**, jamais de crash.

use super::quota_dir;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Une fenetre de quota exact (5h ou 7d) capturee depuis la statusline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowQuota {
    /// Pourcentage utilise 0..100 (champ `used_percentage` de la statusline).
    pub used_percentage: f64,
    /// Epoch **secondes** de la prochaine recharge.
    pub resets_at: i64,
}

/// Les fenetres presentes dans une capture. Une fenetre absente est **omise** (pas `null`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RateLimits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<WindowQuota>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seven_day: Option<WindowQuota>,
}

/// Contenu d'un fichier `quota/<provider>.<account>.json` (schema D4, stable).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaFile {
    pub account: String,
    pub provider: String,
    /// Epoch **secondes** de la capture statusline (sert a juger la fraicheur).
    pub captured_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_version: Option<String>,
    pub rate_limits: RateLimits,
}

impl QuotaFile {
    /// Nom de fichier canonique (`<provider>.<account>.json`), segments assainis (pas de `/`).
    pub fn file_name(provider: &str, account: &str) -> String {
        let sane = |s: &str| s.replace(['/', '\\', '.'], "_");
        format!("{}.{}.json", sane(provider), sane(account))
    }

    /// Ecrit ce fichier quota sous `<home>/quota/` (cree le dossier au besoin).
    pub fn save(&self, home: &Path) -> std::io::Result<()> {
        let dir = quota_dir(home);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(Self::file_name(&self.provider, &self.account));
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }
}

/// Charge tous les fichiers `<home>/quota/*.json`. Un fichier malforme est ignore (+ warning) ;
/// dossier absent -> liste vide. Defensif, jamais de panique.
pub fn load_quota_files(home: &Path) -> Vec<QuotaFile> {
    let dir = quota_dir(home);
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = match std::fs::read_to_string(&path) {
            Ok(r) => r,
            Err(_) => continue,
        };
        match serde_json::from_str::<QuotaFile>(&raw) {
            Ok(q) => out.push(q),
            Err(e) => eprintln!(
                "[iakatc] quota ignore (malforme) : {} ({e})",
                path.display()
            ),
        }
    }
    // Ordre deterministe (provider puis account) pour des publications stables.
    out.sort_by(|a, b| a.provider.cmp(&b.provider).then(a.account.cmp(&b.account)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_home(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("iakatc-store-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_puis_load_roundtrip() {
        let home = tmp_home("rt");
        let q = QuotaFile {
            account: "max".into(),
            provider: "claude".into(),
            captured_at: 1751846100,
            source_version: Some("2.1.90".into()),
            rate_limits: RateLimits {
                five_hour: Some(WindowQuota {
                    used_percentage: 23.5,
                    resets_at: 1751864400,
                }),
                seven_day: Some(WindowQuota {
                    used_percentage: 41.2,
                    resets_at: 1752451200,
                }),
            },
        };
        q.save(&home).unwrap();
        let loaded = load_quota_files(&home);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], q);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn fenetre_absente_est_omise_pas_null() {
        let q = QuotaFile {
            account: "pro".into(),
            provider: "claude".into(),
            captured_at: 10,
            source_version: None,
            rate_limits: RateLimits {
                five_hour: Some(WindowQuota {
                    used_percentage: 5.0,
                    resets_at: 99,
                }),
                seven_day: None,
            },
        };
        let json = serde_json::to_string(&q).unwrap();
        assert!(!json.contains("seven_day"), "fenetre absente omise : {json}");
        assert!(!json.contains("source_version"));
    }

    #[test]
    fn fichier_malforme_est_ignore_sans_crash() {
        let home = tmp_home("bad");
        let dir = quota_dir(&home);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("claude.max.json"), "{ pas du json").unwrap();
        std::fs::write(dir.join("claude.pro.json"), r#"{"account":"pro","provider":"claude","captured_at":1,"rate_limits":{}}"#).unwrap();
        let loaded = load_quota_files(&home);
        assert_eq!(loaded.len(), 1, "le malforme est saute, le valide reste");
        assert_eq!(loaded[0].account, "pro");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn dossier_absent_rend_liste_vide() {
        let home = std::env::temp_dir().join("iakatc-store-absent-xyz");
        assert!(load_quota_files(&home).is_empty());
    }
}
