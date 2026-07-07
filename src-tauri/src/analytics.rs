//! analytics — hook D6 rempli (feature-app-analytics.md). Ouvre la **vraie vue d'historique** pour
//! un compte `(provider, account)` double-clique. **Meme app Tauri, nouvelle fenetre** (D1) : meme
//! backend, meme etat MQTT, acces `iatc-core` via la commande `get_history`. La vue affiche le
//! quota courant du compte en tete (reutilise `get_reservoirs`) + l'historique par provider.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Ouvre (ou refocalise) la vue analytics pour `(provider, account)`.
pub fn open_view(app: &AppHandle, provider: &str, account: &str) -> Result<(), String> {
    let label = format!("analytics-{}-{}", sanitize(provider), sanitize(account));
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.set_focus();
        return Ok(());
    }
    let url = format!(
        "analytics.html?provider={}&account={}",
        encode(provider),
        encode(account)
    );
    WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title(format!("Analytics — {provider} / {account}"))
        .inner_size(920.0, 680.0)
        .min_inner_size(560.0, 420.0)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Label de fenetre sur : conserve [a-z0-9-_], remplace le reste par `_`.
fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "default".to_string()
    } else {
        cleaned
    }
}

/// Encodage minimal pour la query string (les etiquettes de compte/provider sont simples).
fn encode(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32 & 0xFF)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_produit_un_label_sur() {
        assert_eq!(sanitize("max"), "max");
        assert_eq!(sanitize("Team Pro"), "team_pro");
        assert_eq!(sanitize(""), "default");
    }

    #[test]
    fn encode_echappe_les_espaces() {
        assert_eq!(encode("team pro"), "team%20pro");
        assert_eq!(encode("max"), "max");
    }
}
