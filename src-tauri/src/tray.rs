//! tray — icone de barre systeme (D4) : icone simple + tooltip du pire reservoir + clic gauche
//! qui ouvre/masque la popover. Menu clic droit « Ouvrir » / « Quitter ». Pas de dessin fin dans
//! l'icone (neutralise le risque cross-OS) : le detail vit dans la popover.

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::state::{ReservoirCard, Worst};

/// Identifiant du tray, pour le retrouver via `app.tray_by_id`.
pub const TRAY_ID: &str = "main";

/// Construit l'icone tray dans le hook `setup`.
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItemBuilder::with_id("open", "Ouvrir").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quitter").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&open, &quit]).build()?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("iakaTokenCounter — en attente…")
        .menu(&menu)
        // Reserve le clic gauche a la popover (le menu ne s'ouvre qu'au clic droit).
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_popover(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_popover(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder.build(app)?;
    Ok(())
}

/// Met a jour le tooltip du tray = pire reservoir (plus petit remaining_pct), + etat broker.
pub fn update_tooltip(app: &AppHandle, worst: Option<&Worst>, connected: bool) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let suffix = if connected { "" } else { " — broker deconnecte" };
    let text = match worst {
        Some(w) => format!("{} : {:.0} % restant{suffix}", w.label, w.remaining_pct),
        None => format!("iakaTokenCounter — aucune donnee{suffix}"),
    };
    let _ = tray.set_tooltip(Some(text));
}

/// Recompose l'icone du tray = **logo + mini-reservoirs du compte le plus critique** (D2/D3).
/// Icone couleur **non-template** (ne s'inverse pas). Sans donnee, on garde l'icone en place.
pub fn update_icon(app: &AppHandle, cards: &[ReservoirCard]) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let Some(card) = crate::icon::select_worst_account(cards) else {
        return; // aucun compte encore : on conserve l'icone par defaut.
    };
    match crate::icon::render_icon(card) {
        Ok(img) => {
            let _ = tray.set_icon(Some(img));
            // Couleur de marque : surtout pas de mode template (qui la teindrait en monochrome).
            let _ = tray.set_icon_as_template(false);
        }
        Err(e) => eprintln!("[iakatc-tray] rendu de l'icone echoue: {e}"),
    }
}

fn show_popover(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("popover") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn toggle_popover(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("popover") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
}
