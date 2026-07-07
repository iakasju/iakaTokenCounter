//! analytics — hook D6. Ouvre une **fenetre stub « A venir »** pour un compte donne. Ce n'est pas
//! l'app d'analytics (hors scope, 3e instruction) : juste le point d'entree, rendu **observable**.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Ouvre (ou refocalise) la fenetre stub pour `account`.
pub fn open_stub(app: &AppHandle, account: &str) -> Result<(), String> {
    let label = format!("analytics-{}", sanitize(account));
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.set_focus();
        return Ok(());
    }
    let url = format!("analytics.html?account={}", encode(account));
    WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title(format!("Analytics — {account} (a venir)"))
        .inner_size(440.0, 320.0)
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

/// Encodage minimal pour la query string (les etiquettes de compte sont simples).
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
