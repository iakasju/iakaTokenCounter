//! icon — composition + rasterisation RGBA de l'icone de tray (D2/D3 tray-visuals, D6 agents-en-cours).
//!
//! Spec pixel : `docs/design/tray-icon-spec.html`. Canvas **40 × 18**, logo officiel de marque
//! 16×16 a (1,1) + deux mini-reservoirs (pistes 18×5) a droite (5h a y=3, 7j a y=10) ; repli
//! 1 barre centree a y=6.5 si le compte n'a qu'une fenetre. **Ces 40 premiers pixels sont
//! inchanges au pixel pres** (feature-tray-visuals.md D2, qui fait foi).
//!
//! **Extension (D6 de `feature-agents-en-cours.md`)** : le canvas passe a **54 × 18** (rasterise
//! @2x en **108 × 36**). La zone ajoutee a droite (x > 40) porte le **compteur d'agents en cours**
//! (D6/D8) : filet separateur, pastille grise neutre (jamais une teinte carburant : c'est un
//! effectif, pas un niveau), chiffre centre (`1`..`9`, `9+` au-dela). Effectif nul -> zone vide,
//! mais le canvas **reste a 54** de large (pas de saut de largeur a chaque tick, D6).
//!
//! Voie A de la spec : on **compose un SVG** (logo verbatim + rects) puis on le **rasterise** en
//! RGBA via `resvg` (qui re-exporte `usvg` + `tiny_skia`). Le logo reste faithful au trace officiel
//! (marques deposees, usage strictement nominatif — cf. README). Icone **couleur non-template**.
//!
//! Palette (spec) : piste vide `#8E8E93`, ok ≥50 `#34C759`, moyen 20–49 `#FF9F0A`, alerte <20
//! `#FF3B30` ; Claude `#D97757`, OpenAI/Codex `#10A37F`, fallback `#6F6F78`. Mapping largeur :
//! `w = max(2, round(pct/100 × 18))` (liseré minimal 2 px tant que pct > 0). Incertitude sans
//! mentir : `local_estimate` → hachure 45°, `none`/inconnu → piste pointillee (jamais de faux plein).

use resvg::{tiny_skia, usvg};
use tauri::image::Image;

use crate::state::{ReservoirCard, WindowState};

// --- Traces officiels des marques (SVG inline, extraits verbatim de la spec). ------------------
// Marques deposees de leurs proprietaires (Anthropic / OpenAI) — reproduction du trace officiel a
// des fins STRICTEMENT NOMINATIVES d'identification de compte. Cf. note trademark du README.

/// Symbole Anthropic / Claude (« sunburst »), viewBox 24, couleur officielle `#D97757`.
const CLAUDE_PATH: &str = "m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z";

/// Nœud OpenAI / Codex (6 petales, viewBox 2406) — un motif repete par 6 rotations de 60°.
const OPENAI_PATH: &str = "M1107.3 299.1c-197.999 0-373.9 127.3-435.2 315.3L650 743.5v427.9c0 21.4 11 40.4 29.4 51.4l344.5 198.515V833.3h.1v-27.9L1372.7 604c33.715-19.52 70.44-32.857 108.47-39.828L1447.6 450.3C1361 353.5 1237.1 298.5 1107.3 299.1zm0 117.5-.6.6c79.699 0 156.3 27.5 217.6 78.4-2.5 1.2-7.4 4.3-11 6.1L952.8 709.3c-18.4 10.4-29.4 30-29.4 51.4V1248l-155.1-89.4V755.8c-.1-187.099 151.601-338.9 339-339.2z";

// --- Modele de niveau / remplissage (pur, testable sans rasterisation). ------------------------

/// Niveau de remplissage → teinte (seuils spec : ≥50 ok, 20–49 moyen, <20 alerte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Ok,
    Mid,
    Low,
}

impl Level {
    fn hex(self) -> &'static str {
        match self {
            Level::Ok => "#34c759",
            Level::Mid => "#ff9f0a",
            Level::Low => "#ff3b30",
        }
    }
}

/// Teinte selon le % restant.
fn level_for(pct: f64) -> Level {
    if pct >= 50.0 {
        Level::Ok
    } else if pct >= 20.0 {
        Level::Mid
    } else {
        Level::Low
    }
}

/// Largeur de la barre en px @1x : `max(2, round(pct/100 × 18))`, mais **0** si `pct ≤ 0`
/// (un liseré minimal de 2 px reste visible tant que pct > 0 ; a 0 la barre est vide).
fn bar_width(pct: f64) -> f64 {
    if pct <= 0.0 {
        return 0.0;
    }
    (pct / 100.0 * 18.0).round().max(2.0)
}

/// Traitement de surface d'une barre selon la confiance de la donnee (sans mentir).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fill {
    /// `official` (ou pct connu sans code de confiance) : remplissage plein net.
    Solid,
    /// `local_estimate` : remplissage hachure 45° (on voit que c'est approximatif).
    Hatch,
    /// `official_stale` : remplissage attenue (perime mais date).
    Stale,
    /// `none` ou pct inconnu : piste pointillee vide, jamais de remplissage.
    Unknown,
}

/// Classe une fenetre en traitement de surface (mapping confiance → Fill).
fn classify(w: &WindowState) -> Fill {
    match w.remaining_pct {
        None => Fill::Unknown,
        Some(_) => match w.confidence.as_deref() {
            Some("none") => Fill::Unknown,
            Some("local_estimate") => Fill::Hatch,
            Some("official_stale") => Fill::Stale,
            // `official`, absence de code, ou code futur : on a une valeur → plein.
            _ => Fill::Solid,
        },
    }
}

/// Une barre du modele d'icone : position verticale, % restant, traitement.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Bar {
    y: f64,
    pct: f64,
    fill: Fill,
}

/// Modele d'icone : le logo (via provider) + 1 ou 2 barres.
#[derive(Debug, Clone, PartialEq)]
struct IconModel {
    provider: String,
    bars: Vec<Bar>,
}

/// Une fenetre est « presente » des qu'elle a recu au moins un code (updated_at pose).
fn has_window(w: &WindowState) -> bool {
    w.updated_at.is_some()
}

fn bar_from(w: &WindowState, y: f64) -> Bar {
    let pct = w.remaining_pct.unwrap_or(0.0);
    Bar {
        y,
        pct,
        fill: classify(w),
    }
}

/// Construit le modele d'icone d'un compte. Repli 1 barre (y=6.5) si une seule fenetre presente ;
/// sinon 5h a y=3 et 7j a y=10.
fn model_for(card: &ReservoirCard) -> IconModel {
    // Cas Codex free : seule la fenetre 30j porte un vrai quota (5h/7d mesures sans jauge).
    // On evite alors deux barres « inconnu » trompeuses et on rend la seule jauge 30j (repli 1 barre).
    if card.thirty_d.remaining_pct.is_some()
        && card.five_h.remaining_pct.is_none()
        && card.seven_d.remaining_pct.is_none()
    {
        return IconModel {
            provider: card.provider.clone(),
            bars: vec![bar_from(&card.thirty_d, 6.5)],
        };
    }
    let five = has_window(&card.five_h);
    let seven = has_window(&card.seven_d);
    let bars = match (five, seven) {
        (true, true) => vec![bar_from(&card.five_h, 3.0), bar_from(&card.seven_d, 10.0)],
        (true, false) => vec![bar_from(&card.five_h, 6.5)],
        (false, true) => vec![bar_from(&card.seven_d, 6.5)],
        // Aucune fenetre connue : une barre inconnue centree (ne devrait pas arriver).
        (false, false) => vec![Bar {
            y: 6.5,
            pct: 0.0,
            fill: Fill::Unknown,
        }],
    };
    IconModel {
        provider: card.provider.clone(),
        bars,
    }
}

// --- Selection du pire compte (D3). ------------------------------------------------------------

/// Plus petit `remaining_pct` connu du compte (toutes fenetres confondues), `None` si tout inconnu.
fn card_min_remaining(c: &ReservoirCard) -> Option<f64> {
    let mut m: Option<f64> = None;
    for w in [&c.five_h, &c.seven_d, &c.thirty_d] {
        if let Some(p) = w.remaining_pct {
            m = Some(m.map_or(p, |cur: f64| cur.min(p)));
        }
    }
    m
}

/// Compte le plus critique (plus petit % restant). Les comptes tout-inconnu passent en dernier.
/// `None` si la liste est vide. Coder la selection meme avec un seul compte (D3).
pub fn select_worst_account(cards: &[ReservoirCard]) -> Option<&ReservoirCard> {
    cards.iter().min_by(|a, b| {
        let ka = card_min_remaining(a).unwrap_or(f64::INFINITY);
        let kb = card_min_remaining(b).unwrap_or(f64::INFINITY);
        ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
    })
}

// --- Zone "agents en cours" (D6, extension de canvas). ------------------------------------------

/// Largeur totale du canvas (D6) : 40 (reservoirs, inchange) + 14 (separateur + pastille compteur).
const CANVAS_W: u32 = 54;
const CANVAS_H: u32 = 18;

/// Texte du badge d'effectif (D6) : `0` -> vide (zone non dessinee), `1..=9` -> le chiffre tel
/// quel, au-dela -> `9+` (jamais plus large que 2 caracteres, la pastille est fixe a 12×12).
fn count_badge_text(count: u32) -> String {
    match count {
        0 => String::new(),
        1..=9 => count.to_string(),
        _ => "9+".to_string(),
    }
}

/// SVG de la zone "agents en cours" (filet separateur + pastille de compteur, D6). Chaine vide si
/// l'effectif est nul (D6 : "la zone est vide (rien de dessine)", mais le canvas reste a 54).
fn agents_zone_svg(count: u32) -> String {
    let text = count_badge_text(count);
    if text.is_empty() {
        return String::new();
    }
    let font_size = if text.len() > 1 { 8.0 } else { 9.5 };
    format!(
        r##"<rect x="40.5" y="4" width="1" height="10" fill="#8e8e93" fill-opacity="0.5"/><rect x="41" y="3" width="12" height="12" rx="3.5" fill="#6f6f78"/><text x="47" y="12.2" text-anchor="middle" font-family="-apple-system,Helvetica,sans-serif" font-size="{font_size}" font-weight="700" fill="#f3f3f6">{text}</text>"##
    )
}

// --- Composition SVG. --------------------------------------------------------------------------

/// Logo officiel (SVG inline) selon le provider, place a (1,1) en 16×16. Fallback = pastille neutre
/// `#6F6F78` + initiale majuscule du provider.
fn logo_svg(provider: &str) -> String {
    match provider.to_ascii_lowercase().as_str() {
        "claude" | "anthropic" => format!(
            r##"<svg x="1" y="1" width="16" height="16" viewBox="0 0 24 24"><path d="{CLAUDE_PATH}" fill="#D97757"/></svg>"##
        ),
        "codex" | "openai" | "chatgpt" | "gpt" => {
            let mut paths = format!(r#"<path d="{OPENAI_PATH}"/>"#);
            for a in [60, 120, 180, 240, 300] {
                paths.push_str(&format!(
                    r#"<path d="{OPENAI_PATH}" transform="rotate({a} 1203 1203)"/>"#
                ));
            }
            format!(
                r##"<svg x="1" y="1" width="16" height="16" viewBox="0 0 2406 2406"><g fill="#10a37f">{paths}</g></svg>"##
            )
        }
        other => {
            let initial = other
                .chars()
                .next()
                .map(|c| c.to_ascii_uppercase())
                .unwrap_or('?');
            format!(
                r##"<svg x="1" y="1" width="16" height="16" viewBox="0 0 24 24"><rect x="2" y="2" width="20" height="20" rx="6" fill="#6f6f78"/><text x="12" y="16.5" text-anchor="middle" font-family="-apple-system,Helvetica,sans-serif" font-size="12" font-weight="700" fill="#f3f3f6">{initial}</text></svg>"##
            )
        }
    }
}

/// SVG d'une barre (piste + remplissage) ; alimente `defs` pour la hachure des estimations.
fn bar_svg(bar: &Bar, defs: &mut String, idx: usize) -> String {
    let y = bar.y;
    if bar.fill == Fill::Unknown {
        // Inconnu : piste pointillee vide, aucun remplissage (jamais de faux plein).
        return format!(
            r##"<rect x="20.4" y="{:.2}" width="17.2" height="4.2" rx="2.1" fill="none" stroke="#8e8e93" stroke-width="0.9" stroke-dasharray="2 1.6"/>"##,
            y + 0.4
        );
    }
    // Piste vide en gris moyen (contraste sur clair ET sombre).
    let mut s = format!(r##"<rect x="20" y="{y}" width="18" height="5" rx="2.5" fill="#8e8e93"/>"##);
    let w = bar_width(bar.pct);
    if w <= 0.0 {
        return s; // pct 0 : piste vide seule.
    }
    let level = level_for(bar.pct);
    let paint = if bar.fill == Fill::Hatch {
        let id = format!("hatch{idx}");
        defs.push_str(&format!(
            r##"<pattern id="{id}" width="3" height="3" patternTransform="rotate(45)" patternUnits="userSpaceOnUse"><rect width="3" height="3" fill="{}"/><rect width="1.5" height="3" fill="#ffffff" fill-opacity="0.5"/></pattern>"##,
            level.hex()
        ));
        format!("url(#{id})")
    } else {
        level.hex().to_string()
    };
    let opacity = if bar.fill == Fill::Stale {
        r#" fill-opacity="0.45""#
    } else {
        ""
    };
    s.push_str(&format!(
        r#"<rect x="20" y="{y}" width="{w}" height="5" rx="2.5" fill="{paint}"{opacity}/>"#
    ));
    s
}

/// Compose le SVG complet 54×18 (logo + barres + zone agents, D6) d'un compte. Les 40 premiers
/// pixels (logo + barres) sont inchanges au pixel pres (feature-tray-visuals.md D2) ; `agent_count`
/// alimente uniquement la zone ajoutee a droite (D6).
fn compose_svg(card: &ReservoirCard, agent_count: u32) -> String {
    let model = model_for(card);
    let logo = logo_svg(&model.provider);
    let mut defs = String::new();
    let mut body = String::new();
    for (i, bar) in model.bars.iter().enumerate() {
        body.push_str(&bar_svg(bar, &mut defs, i));
    }
    let agents_zone = agents_zone_svg(agent_count);
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{CANVAS_W}" height="{CANVAS_H}" viewBox="0 0 {CANVAS_W} {CANVAS_H}"><defs>{defs}</defs>{logo}{body}{agents_zone}</svg>"#
    )
}

// --- Rasterisation. ----------------------------------------------------------------------------

/// Facteur retina : on rasterise le canvas 54×18 (D6) en **108 × 36** (@2x) ; macOS mettra a
/// l'echelle.
const SCALE: f32 = 2.0;
const OUT_W: u32 = 108;
const OUT_H: u32 = 36;

/// Rasterise un SVG compose en RGBA droit (non-premultiplie) `(pixels, largeur, hauteur)`.
/// `agent_count` alimente la zone "agents en cours" ajoutee a droite (D6).
pub fn render_rgba(card: &ReservoirCard, agent_count: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let svg = compose_svg(card, agent_count);
    let mut opt = usvg::Options::default();
    // Fallback : rendu de l'initiale (police systeme). Charge une fois par appel (rare : fallback
    // seul cas ayant du texte). Les logos Claude/OpenAI sont des paths purs, sans police.
    opt.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_str(&svg, &opt).map_err(|e| format!("usvg parse: {e}"))?;
    let mut pixmap = tiny_skia::Pixmap::new(OUT_W, OUT_H).ok_or("pixmap alloc")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(SCALE, SCALE),
        &mut pixmap.as_mut(),
    );
    let mut rgba = Vec::with_capacity((OUT_W * OUT_H * 4) as usize);
    for px in pixmap.pixels() {
        let c = px.demultiply();
        rgba.push(c.red());
        rgba.push(c.green());
        rgba.push(c.blue());
        rgba.push(c.alpha());
    }
    Ok((rgba, OUT_W, OUT_H))
}

/// Rasterise l'icone d'un compte en `tauri::image::Image` prete pour `tray.set_icon`. `agent_count`
/// = effectif d'agents en cours a afficher dans la zone ajoutee a droite (D6).
pub fn render_icon(card: &ReservoirCard, agent_count: u32) -> Result<Image<'static>, String> {
    let (rgba, w, h) = render_rgba(card, agent_count)?;
    Ok(Image::new_owned(rgba, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(remaining: Option<f64>, conf: Option<&str>, present: bool) -> WindowState {
        WindowState {
            remaining_pct: remaining,
            confidence: conf.map(str::to_string),
            updated_at: present.then_some(1),
            ..Default::default()
        }
    }

    fn card(provider: &str, five: WindowState, seven: WindowState) -> ReservoirCard {
        ReservoirCard {
            provider: provider.to_string(),
            account: "x".to_string(),
            five_h: five,
            seven_d: seven,
            thirty_d: WindowState::default(),
        }
    }

    #[test]
    fn mapping_pourcent_vers_largeur() {
        assert_eq!(bar_width(0.0), 0.0); // vide : aucune barre
        assert_eq!(bar_width(1.0), 2.0); // >0 : plancher 2 px
        assert_eq!(bar_width(10.0), 2.0); // <20 : plancher 2 px (round(1.8)=2)
        assert_eq!(bar_width(41.0), 7.0); // 20–49 : round(7.38)=7
        assert_eq!(bar_width(50.0), 9.0); // ≥50 : round(9.0)=9
        assert_eq!(bar_width(100.0), 18.0); // plein
    }

    #[test]
    fn teinte_par_niveau() {
        assert_eq!(level_for(73.0), Level::Ok);
        assert_eq!(level_for(50.0), Level::Ok); // borne haute
        assert_eq!(level_for(49.9), Level::Mid);
        assert_eq!(level_for(20.0), Level::Mid); // borne basse moyen
        assert_eq!(level_for(19.9), Level::Low);
        assert_eq!(level_for(8.0), Level::Low);
    }

    #[test]
    fn traitement_par_confiance() {
        assert_eq!(classify(&ws(Some(70.0), Some("official"), true)), Fill::Solid);
        assert_eq!(
            classify(&ws(Some(40.0), Some("local_estimate"), true)),
            Fill::Hatch
        );
        assert_eq!(
            classify(&ws(Some(40.0), Some("official_stale"), true)),
            Fill::Stale
        );
        assert_eq!(classify(&ws(None, Some("none"), true)), Fill::Unknown);
        assert_eq!(classify(&ws(Some(40.0), Some("none"), true)), Fill::Unknown);
        // pct connu sans code de confiance : on a une valeur → plein.
        assert_eq!(classify(&ws(Some(40.0), None, true)), Fill::Solid);
    }

    #[test]
    fn repli_une_barre() {
        // Une seule fenetre presente → une barre centree a y=6.5.
        let c1 = card(
            "claude",
            ws(Some(62.0), Some("official"), true),
            ws(None, None, false),
        );
        let m1 = model_for(&c1);
        assert_eq!(m1.bars.len(), 1);
        assert_eq!(m1.bars[0].y, 6.5);
        // Deux fenetres → deux barres (y=3 et y=10).
        let c2 = card(
            "claude",
            ws(Some(80.0), Some("official"), true),
            ws(Some(40.0), Some("official"), true),
        );
        let m2 = model_for(&c2);
        assert_eq!(m2.bars.len(), 2);
        assert_eq!(m2.bars[0].y, 3.0);
        assert_eq!(m2.bars[1].y, 10.0);
    }

    #[test]
    fn codex_free_rend_une_barre_30j() {
        // 5h/7d mesures sans jauge (branche 4), 30j = seule vraie jauge.
        let mut c = card(
            "codex",
            ws(None, Some("none"), true),
            ws(None, Some("none"), true),
        );
        c.thirty_d = ws(Some(37.0), Some("official"), true);
        let m = model_for(&c);
        assert_eq!(m.bars.len(), 1, "une seule barre : la jauge 30j");
        assert_eq!(m.bars[0].y, 6.5);
        // La jauge 30j alimente aussi la selection du pire compte.
        assert_eq!(card_min_remaining(&c), Some(37.0));
    }

    #[test]
    fn selection_pire_compte() {
        let claude = card(
            "claude",
            ws(Some(80.0), Some("official"), true),
            ws(Some(42.0), Some("official"), true),
        );
        let codex = card(
            "codex",
            ws(Some(14.0), Some("official"), true),
            ws(None, None, false),
        );
        let two = [claude, codex];
        let worst = select_worst_account(&two).unwrap();
        assert_eq!(worst.provider, "codex"); // 14 < 42

        // Un seul compte : selection triviale.
        let one = vec![card(
            "claude",
            ws(Some(90.0), Some("official"), true),
            ws(None, None, false),
        )];
        assert_eq!(select_worst_account(&one).unwrap().provider, "claude");

        // Aucun compte.
        assert!(select_worst_account(&[]).is_none());
    }

    #[test]
    fn compose_svg_contient_logo_et_teintes() {
        let c = card(
            "claude",
            ws(Some(73.0), Some("official"), true),
            ws(Some(41.0), Some("official"), true),
        );
        // Canvas etendu a 54×18 (D6 feature-agents-en-cours.md) ; les 40 premiers pixels (logo +
        // barres) restent inchanges au pixel pres (feature-tray-visuals.md D2).
        let svg = compose_svg(&c, 0);
        assert!(svg.contains(r#"viewBox="0 0 54 18""#));
        assert!(svg.contains("#D97757")); // logo Claude
        assert!(svg.contains("#34c759")); // 5h ≥50 → vert
        assert!(svg.contains("#ff9f0a")); // 7j 20–49 → ambre
    }

    #[test]
    fn compose_svg_inconnu_est_pointille_sans_remplissage() {
        let c = card(
            "codex",
            ws(Some(14.0), Some("local_estimate"), true),
            ws(None, Some("none"), true),
        );
        let svg = compose_svg(&c, 0);
        assert!(svg.contains("stroke-dasharray")); // 7j inconnu → piste pointillee
        assert!(svg.contains("url(#hatch")); // 5h estime → hachure
        assert!(svg.contains("#10a37f")); // logo OpenAI/Codex
    }

    #[test]
    fn rendu_rgba_produit_une_image_108x36() {
        let c = card(
            "claude",
            ws(Some(73.0), Some("official"), true),
            ws(Some(41.0), Some("official"), true),
        );
        let img = render_icon(&c, 0).expect("rasterisation ok");
        assert_eq!(img.width(), OUT_W);
        assert_eq!(img.height(), OUT_H);
    }

    // ---------------- D6 (feature-agents-en-cours.md) : zone "agents en cours" ----------------

    #[test]
    fn badge_texte_zero_vide_un_a_neuf_tel_quel_dix_plus_neuf_plus() {
        assert_eq!(count_badge_text(0), "");
        assert_eq!(count_badge_text(1), "1");
        assert_eq!(count_badge_text(9), "9");
        assert_eq!(count_badge_text(10), "9+");
        assert_eq!(count_badge_text(99), "9+");
    }

    #[test]
    fn zone_agents_vide_quand_effectif_nul() {
        assert_eq!(agents_zone_svg(0), "");
    }

    #[test]
    fn zone_agents_dessine_le_badge_quand_effectif_non_nul() {
        let svg1 = agents_zone_svg(1);
        assert!(svg1.contains(">1<"));
        let svg10 = agents_zone_svg(10);
        assert!(svg10.contains(">9+<"));
    }

    #[test]
    fn canvas_reste_a_54_de_large_quel_que_soit_l_effectif() {
        let c = card(
            "claude",
            ws(Some(73.0), Some("official"), true),
            ws(None, None, false),
        );
        for count in [0u32, 1, 9, 10, 99] {
            let svg = compose_svg(&c, count);
            assert!(
                svg.contains(r#"width="54""#) && svg.contains(r#"viewBox="0 0 54 18""#),
                "canvas doit rester 54×18 pour effectif={count}: {svg}"
            );
        }
    }
}
