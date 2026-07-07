//! statusline — sous-commande `statusline-capture` (D4).
//!
//! Branchee comme statusline de Claude Code : lit le **JSON statusline sur stdin**, et si
//! `rate_limits` est present, **persiste** un fichier `quota/<provider>.<account>.json` sous
//! `IAKATC_HOME`. Re-emet une **ligne d'affichage minimale sur stdout** (pass-through) et **ne
//! plante jamais** (echec silencieux, code 0) — la statusline reste fonctionnelle.

use iakatc_core::now_epoch_s;
use iakatc_core::quota::store::{QuotaFile, RateLimits, WindowQuota};
use serde_json::Value;
use std::io::Read;

/// Provider fixe de la statusline Claude Code.
const PROVIDER: &str = "claude";

/// Point d'entree de la sous-commande. Ne renvoie jamais d'erreur (toujours code 0 cote appelant).
pub fn run(account_label: &str) {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        // stdin illisible : on n'ecrit rien, on ne casse pas la statusline.
        println!("iakatc");
        return;
    }
    let json: Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(_) => {
            // JSON invalide : pass-through minimal, pas d'ecriture.
            println!("iakatc");
            return;
        }
    };

    let rate_limits = extract_rate_limits(&json);
    let has_data = rate_limits.five_hour.is_some() || rate_limits.seven_day.is_some();

    if has_data {
        let file = QuotaFile {
            account: account_label.to_string(),
            provider: PROVIDER.to_string(),
            captured_at: now_epoch_s(),
            source_version: json
                .get("version")
                .and_then(Value::as_str)
                .map(str::to_string),
            rate_limits: rate_limits.clone(),
        };
        if let Some(home) = iakatc_core::quota::resolve_home() {
            if let Err(e) = file.save(&home) {
                eprintln!("[iakatc] capture quota non ecrite ({e})");
            }
        }
    }

    // Ligne d'affichage minimale (ne casse pas la statusline).
    println!("{}", render_line(account_label, &rate_limits));
}

/// Extrait `rate_limits.{five_hour,seven_day}.{used_percentage,resets_at}` de facon defensive.
fn extract_rate_limits(json: &Value) -> RateLimits {
    let rl = json.get("rate_limits");
    RateLimits {
        five_hour: rl.and_then(|r| window(r.get("five_hour"))),
        seven_day: rl.and_then(|r| window(r.get("seven_day"))),
    }
}

/// Lit une fenetre `{used_percentage, resets_at}` — `None` si l'un des champs manque.
fn window(w: Option<&Value>) -> Option<WindowQuota> {
    let w = w?;
    let used = w.get("used_percentage").and_then(Value::as_f64)?;
    let resets_at = w.get("resets_at").and_then(Value::as_i64)?;
    Some(WindowQuota {
        used_percentage: used,
        resets_at,
    })
}

/// Rend une ligne compacte pour la statusline (ex. `iakatc max 5h:23% 7d:41%`).
fn render_line(account: &str, rl: &RateLimits) -> String {
    let mut parts = vec![format!("iakatc {account}")];
    if let Some(w) = &rl.five_hour {
        parts.push(format!("5h:{:.0}%", w.used_percentage));
    }
    if let Some(w) = &rl.seven_day {
        parts.push(format!("7d:{:.0}%", w.used_percentage));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrait_les_deux_fenetres() {
        let json: Value = serde_json::from_str(
            r#"{"version":"2.1.90","rate_limits":{"five_hour":{"used_percentage":23.5,"resets_at":1751864400},"seven_day":{"used_percentage":41.2,"resets_at":1752451200}}}"#,
        )
        .unwrap();
        let rl = extract_rate_limits(&json);
        assert_eq!(rl.five_hour.as_ref().unwrap().used_percentage, 23.5);
        assert_eq!(rl.seven_day.as_ref().unwrap().resets_at, 1752451200);
    }

    #[test]
    fn sans_rate_limits_rien_a_ecrire() {
        let json: Value = serde_json::from_str(r#"{"model":{"id":"claude"}}"#).unwrap();
        let rl = extract_rate_limits(&json);
        assert!(rl.five_hour.is_none() && rl.seven_day.is_none());
    }

    #[test]
    fn fenetre_incomplete_ignoree() {
        // used_percentage sans resets_at -> fenetre ecartee (defensif).
        let json: Value =
            serde_json::from_str(r#"{"rate_limits":{"five_hour":{"used_percentage":10.0}}}"#)
                .unwrap();
        assert!(extract_rate_limits(&json).five_hour.is_none());
    }

    #[test]
    fn ligne_minimale_rendue() {
        let rl = RateLimits {
            five_hour: Some(WindowQuota {
                used_percentage: 23.5,
                resets_at: 1,
            }),
            seven_day: None,
        };
        assert_eq!(render_line("max", &rl), "iakatc max 5h:24%");
    }
}
