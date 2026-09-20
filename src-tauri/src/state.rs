//! state — etat en memoire de la GUI : agregation des codes retained en `ReservoirCard[]`.
//!
//! **Coeur testable, sans broker.** On alimente le store par des couples `(topic, payload)`
//! (comme s'ils venaient d'un `Publish` MQTT) et on obtient l'instantane rendu par la webview.
//! La webview ne touche jamais MQTT : elle recoit ce `StateSnapshot` (commande + evenement).
//!
//! Regles :
//! - topic = code du contrat (§ 2) ; payload = `{"v":<scalaire>,"t":<epoch_s>}` (§ 3).
//! - un payload malforme (pas `{v,t}`) est **ignore** sans planter.
//! - `v:null` => la donnee correspondante repasse a `None` (« inconnu date »).
//! - decouverte dynamique des comptes via le wildcard `all/ia/+/+/quota/#`.

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::agents::AgentsSnapshot;
use crate::memory::MemoryLog;
use crate::quota_history::QuotaLog;
use crate::rollups::RollupsLog;

/// Fenetre de quota consommee par la GUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    FiveHour,
    SevenDay,
    /// Fenetre 30 j : plan free Codex (extension additive du contrat, cf. § 2 v1.1).
    ThirtyDay,
}

impl Window {
    fn from_code(s: &str) -> Option<Window> {
        match s {
            "5h" => Some(Window::FiveHour),
            "7d" => Some(Window::SevenDay),
            "30d" => Some(Window::ThirtyDay),
            _ => None,
        }
    }

    /// Code stable de la fenetre (miroir de `from_code`) — utilise par `quota_history` pour
    /// persister la serie sous son code du contrat plutot que sous le nom Rust de la variante.
    pub fn code(self) -> &'static str {
        match self {
            Window::FiveHour => "5h",
            Window::SevenDay => "7d",
            Window::ThirtyDay => "30d",
        }
    }
}

/// Etat d'une fenetre de quota (serialise en camelCase pour la webview).
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub used_pct: Option<f64>,
    pub remaining_pct: Option<f64>,
    pub used_tokens: Option<u64>,
    pub resets_at: Option<i64>,
    pub captured_at: Option<i64>,
    pub confidence: Option<String>,
    pub source: Option<String>,
    /// Epoch s de la valeur la plus fraiche recue pour cette fenetre (fraicheur locale).
    pub updated_at: Option<i64>,
}

impl WindowState {
    /// Applique un code scalaire du contrat. Retourne `true` si l'etat a change.
    fn set_code(&mut self, code: &str, v: &Value, t: i64) -> bool {
        let before = self.clone();
        match code {
            "used_pct" => self.used_pct = v.as_f64(),
            "remaining_pct" => self.remaining_pct = v.as_f64(),
            "used_tokens" => self.used_tokens = v.as_u64(),
            "resets_at" => self.resets_at = v.as_i64(),
            "captured_at" => self.captured_at = v.as_i64(),
            "confidence" => self.confidence = v.as_str().map(str::to_string),
            "source" => self.source = v.as_str().map(str::to_string),
            _ => return false, // code inconnu (ex. code futur) : ignore.
        }
        // Fraicheur : on retient le t le plus recent vu pour la fenetre.
        self.updated_at = Some(self.updated_at.map_or(t, |cur| cur.max(t)));
        *self != before
    }
}

/// Carte de reservoir = un compte IA (provider + account), deux fenetres.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReservoirCard {
    pub provider: String,
    pub account: String,
    pub five_h: WindowState,
    pub seven_d: WindowState,
    /// Fenetre 30 j (Codex free) — vide pour les autres providers.
    pub thirty_d: WindowState,
}

/// Pire reservoir (plus petit remaining_pct connu) — sert au tooltip du tray.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Worst {
    pub label: String,
    pub remaining_pct: f64,
}

/// Instantane pousse a la webview.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StateSnapshot {
    pub reservoirs: Vec<ReservoirCard>,
    pub broker_connected: bool,
    pub daemon_available: bool,
    pub worst: Option<Worst>,
}

#[derive(Debug, Clone, Default)]
struct Card {
    five_h: WindowState,
    seven_d: WindowState,
    thirty_d: WindowState,
}

/// Store des cartes, indexe par `(provider, account)` (ordonne pour un rendu stable).
#[derive(Debug, Default)]
pub struct ReservoirStore {
    cards: BTreeMap<(String, String), Card>,
}

/// Decompose un topic de quota en `(provider, account, window, code)`.
///
/// Attendu : `{root}/all/ia/{provider}/{account}/quota/{5h|7d|30d}/{code}/current`.
/// Tout ce qui ne colle pas (conso, meta, suffixe != current) renvoie `None`.
fn parse_quota_topic(root: &str, topic: &str) -> Option<(String, String, Window, String)> {
    let prefix = format!("{root}/all/ia/");
    let rest = topic.strip_prefix(&prefix)?;
    let parts: Vec<&str> = rest.split('/').collect();
    // provider / account / "quota" / window / code / "current"
    if parts.len() != 6 || parts[2] != "quota" || parts[5] != "current" {
        return None;
    }
    let window = Window::from_code(parts[3])?;
    Some((
        parts[0].to_string(),
        parts[1].to_string(),
        window,
        parts[4].to_string(),
    ))
}

/// Parse le payload `{"v":...,"t":...}`. Payload malforme => `None` (ignore).
fn parse_payload(payload: &[u8]) -> Option<(Value, i64)> {
    let val: Value = serde_json::from_slice(payload).ok()?;
    let obj = val.as_object()?;
    let v = obj.get("v")?.clone();
    let t = obj.get("t")?.as_i64()?;
    Some((v, t))
}

impl ReservoirStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Applique un message MQTT de quota. Retourne `true` si l'etat rendu a change.
    /// Un topic hors-quota ou un payload malforme est ignore (`false`).
    pub fn apply_message(&mut self, root: &str, topic: &str, payload: &[u8]) -> bool {
        let Some((provider, account, window, code)) = parse_quota_topic(root, topic) else {
            return false;
        };
        let Some((v, t)) = parse_payload(payload) else {
            return false;
        };
        let card = self.cards.entry((provider, account)).or_default();
        let ws = match window {
            Window::FiveHour => &mut card.five_h,
            Window::SevenDay => &mut card.seven_d,
            Window::ThirtyDay => &mut card.thirty_d,
        };
        ws.set_code(&code, &v, t)
    }

    /// Cartes ordonnees (provider, account) pour un rendu stable.
    pub fn cards(&self) -> Vec<ReservoirCard> {
        self.cards
            .iter()
            .map(|((provider, account), c)| ReservoirCard {
                provider: provider.clone(),
                account: account.clone(),
                five_h: c.five_h.clone(),
                seven_d: c.seven_d.clone(),
                thirty_d: c.thirty_d.clone(),
            })
            .collect()
    }

    /// Pire reservoir : plus petit `remaining_pct` connu parmi toutes les fenetres.
    pub fn worst(&self) -> Option<Worst> {
        let mut worst: Option<Worst> = None;
        for ((provider, account), c) in &self.cards {
            for (win, ws) in [
                ("5h", &c.five_h),
                ("7d", &c.seven_d),
                ("30d", &c.thirty_d),
            ] {
                if let Some(pct) = ws.remaining_pct {
                    if worst.as_ref().is_none_or(|w| pct < w.remaining_pct) {
                        worst = Some(Worst {
                            label: format!("{provider} {account} {win}"),
                            remaining_pct: pct,
                        });
                    }
                }
            }
        }
        worst
    }
}

/// Etat applicatif partage (manage Tauri) : store + drapeaux broker/daemon.
pub struct AppState {
    pub store: Mutex<ReservoirStore>,
    pub broker_connected: AtomicBool,
    pub daemon_available: AtomicBool,
    /// Enfant du daemon spawne en sidecar (tue a la fermeture de la GUI, D1).
    pub daemon_child: Mutex<Option<tauri_plugin_shell::process::CommandChild>>,
    /// Log memoire persistant (chemin resolu au `setup`). Le `Mutex` serialise sampler vs commande.
    pub memory: Mutex<MemoryLog>,
    /// Historique de quota persistant, 90 j (feature-memoire-historique.md, L1). Meme patron que
    /// `memory` : le `Mutex` serialise le thread sampler et la commande de lecture.
    pub quota_history: Mutex<QuotaLog>,
    /// Rollups quotidiens persistants, sans limite (feature-memoire-historique.md, L1). Le `Mutex`
    /// serialise le declenchement a l'ouverture de la vue analytics, le thread quotidien de fond,
    /// et la commande de lecture.
    pub rollups: Mutex<RollupsLog>,
    /// Instantane des agents Claude Code en cours (feature-agents-en-cours.md, D8). Champ ajoute
    /// EN DERNIER (anti-collision avec le lot L1 ci-dessus) : le `Mutex` serialise le watcher
    /// dedie (ecrit a chaque tick 5 s) et la commande `get_running_agents` (lit).
    pub running_agents: Mutex<AgentsSnapshot>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            store: Mutex::new(ReservoirStore::new()),
            broker_connected: AtomicBool::new(false),
            daemon_available: AtomicBool::new(false),
            daemon_child: Mutex::new(None),
            // Chemin resolu au `setup` via `app_data_dir` (cf. lib.rs) ; vide avant.
            memory: Mutex::new(MemoryLog::default()),
            quota_history: Mutex::new(QuotaLog::default()),
            rollups: Mutex::new(RollupsLog::default()),
            // Vide avant le premier tick du watcher (feature-agents-en-cours.md, D8) : initialise
            // EN DERNIER, meme discipline que le champ ci-dessus.
            running_agents: Mutex::new(AgentsSnapshot::default()),
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construit l'instantane courant pour la webview.
    pub fn snapshot(&self) -> StateSnapshot {
        let store = self.store.lock().unwrap();
        StateSnapshot {
            reservoirs: store.cards(),
            broker_connected: self.broker_connected.load(Ordering::Relaxed),
            daemon_available: self.daemon_available.load(Ordering::Relaxed),
            worst: store.worst(),
        }
    }
}

/// Commande : instantane initial a l'ouverture de la popover.
#[tauri::command]
pub fn get_reservoirs(state: tauri::State<'_, AppState>) -> StateSnapshot {
    state.snapshot()
}

/// Commande : hook analytics (D6, rempli par feature-app-analytics). Ouvre la vue d'historique
/// pour le compte `(provider, account)` double-clique. Le filtrage de l'historique se fait par
/// **provider** (les logs ne portent pas l'ID de compte, D4) ; le quota en tete reste par compte.
///
/// Point de declenchement des rollups quotidiens (D5/etape 5 de `feature-memoire-historique.md`,
/// L1) : « a l'ouverture de la vue analytics (deja un point de rafraichissement existant) » — pas
/// de polling supplementaire ajoute, on branche sur cette commande deja invoquee par la webview
/// (`main.ts`) a chaque double-clic. Best-effort : ne bloque ni n'echoue l'ouverture de la vue.
#[tauri::command]
pub fn open_analytics(app: tauri::AppHandle, provider: String, account: String) -> Result<(), String> {
    crate::rollups::refresh_on_view_open(&app);
    crate::analytics::open_view(&app, &provider, &account)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "iakatokencounter";
    const T: i64 = 1751894400;

    fn payload(v: &str) -> Vec<u8> {
        format!(r#"{{"v":{v},"t":{T}}}"#).into_bytes()
    }

    fn apply(store: &mut ReservoirStore, topic: &str, v: &str) -> bool {
        store.apply_message(ROOT, topic, &payload(v))
    }

    #[test]
    fn decouvre_un_compte_et_remplit_la_jauge_5h() {
        let mut s = ReservoirStore::new();
        let changed = apply(
            &mut s,
            "iakatokencounter/all/ia/claude/max/quota/5h/remaining_pct/current",
            "87.5",
        );
        assert!(changed);
        let cards = s.cards();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].provider, "claude");
        assert_eq!(cards[0].account, "max");
        assert_eq!(cards[0].five_h.remaining_pct, Some(87.5));
        assert_eq!(cards[0].seven_d.remaining_pct, None);
    }

    #[test]
    fn deux_fenetres_sur_le_meme_compte() {
        let mut s = ReservoirStore::new();
        apply(
            &mut s,
            "iakatokencounter/all/ia/claude/max/quota/5h/remaining_pct/current",
            "80",
        );
        apply(
            &mut s,
            "iakatokencounter/all/ia/claude/max/quota/7d/remaining_pct/current",
            "42",
        );
        let cards = s.cards();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].five_h.remaining_pct, Some(80.0));
        assert_eq!(cards[0].seven_d.remaining_pct, Some(42.0));
    }

    #[test]
    fn confiance_et_source_mappees() {
        let mut s = ReservoirStore::new();
        apply(
            &mut s,
            "iakatokencounter/all/ia/claude/max/quota/5h/confidence/current",
            r#""official""#,
        );
        apply(
            &mut s,
            "iakatokencounter/all/ia/claude/max/quota/5h/source/current",
            r#""statusline""#,
        );
        let c = &s.cards()[0].five_h;
        assert_eq!(c.confidence.as_deref(), Some("official"));
        assert_eq!(c.source.as_deref(), Some("statusline"));
    }

    #[test]
    fn v_null_repasse_a_none() {
        let mut s = ReservoirStore::new();
        apply(
            &mut s,
            "iakatokencounter/all/ia/claude/default/quota/7d/remaining_pct/current",
            "55",
        );
        apply(
            &mut s,
            "iakatokencounter/all/ia/claude/default/quota/7d/remaining_pct/current",
            "null",
        );
        assert_eq!(s.cards()[0].seven_d.remaining_pct, None);
    }

    #[test]
    fn multi_comptes_produit_plusieurs_cartes() {
        let mut s = ReservoirStore::new();
        apply(
            &mut s,
            "iakatokencounter/all/ia/claude/max/quota/5h/remaining_pct/current",
            "12",
        );
        apply(
            &mut s,
            "iakatokencounter/all/ia/claude/pro/quota/5h/remaining_pct/current",
            "90",
        );
        apply(
            &mut s,
            "iakatokencounter/all/ia/codex/default/quota/5h/remaining_pct/current",
            "70",
        );
        assert_eq!(s.cards().len(), 3);
    }

    #[test]
    fn payload_malforme_ignore_sans_planter() {
        let mut s = ReservoirStore::new();
        // Pas d'objet {v,t}.
        assert!(!s.apply_message(
            ROOT,
            "iakatokencounter/all/ia/claude/max/quota/5h/remaining_pct/current",
            b"pas du json",
        ));
        // Objet sans t.
        assert!(!s.apply_message(
            ROOT,
            "iakatokencounter/all/ia/claude/max/quota/5h/remaining_pct/current",
            br#"{"v":50}"#,
        ));
        assert_eq!(s.cards().len(), 0);
    }

    #[test]
    fn topic_hors_quota_ignore() {
        let mut s = ReservoirStore::new();
        // Conso (axe ia/agents) : ne doit pas creer de carte.
        assert!(!apply(
            &mut s,
            "iakatokencounter/all/ia/agents/codex/coordinator/conso/used_tokens/current",
            "45000",
        ));
        // Meta daemon.
        assert!(!s.apply_message(
            ROOT,
            "iakatokencounter/meta/daemon/state/current",
            br#"{"v":"up","t":1751894400}"#,
        ));
        assert_eq!(s.cards().len(), 0);
    }

    #[test]
    fn pire_reservoir_est_le_plus_petit_remaining() {
        let mut s = ReservoirStore::new();
        apply(
            &mut s,
            "iakatokencounter/all/ia/claude/max/quota/5h/remaining_pct/current",
            "12",
        );
        apply(
            &mut s,
            "iakatokencounter/all/ia/claude/max/quota/7d/remaining_pct/current",
            "80",
        );
        apply(
            &mut s,
            "iakatokencounter/all/ia/codex/default/quota/5h/remaining_pct/current",
            "40",
        );
        let w = s.worst().expect("un pire reservoir existe");
        assert_eq!(w.remaining_pct, 12.0);
        assert_eq!(w.label, "claude max 5h");
    }

    #[test]
    fn from_code_apprend_30d() {
        assert_eq!(Window::from_code("30d"), Some(Window::ThirtyDay));
        assert_eq!(Window::from_code("5h"), Some(Window::FiveHour));
        assert_eq!(Window::from_code("7d"), Some(Window::SevenDay));
        assert_eq!(Window::from_code("99y"), None);
    }

    #[test]
    fn topic_30d_alimente_thirty_d() {
        let mut s = ReservoirStore::new();
        assert!(apply(
            &mut s,
            "iakatokencounter/all/ia/codex/default/quota/30d/remaining_pct/current",
            "37",
        ));
        let cards = s.cards();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].provider, "codex");
        assert_eq!(cards[0].thirty_d.remaining_pct, Some(37.0));
        // Les autres fenetres restent vides pour ce compte.
        assert_eq!(cards[0].five_h.remaining_pct, None);
        assert_eq!(cards[0].seven_d.remaining_pct, None);
    }

    #[test]
    fn pire_reservoir_peut_etre_porte_par_30d() {
        let mut s = ReservoirStore::new();
        apply(
            &mut s,
            "iakatokencounter/all/ia/claude/max/quota/5h/remaining_pct/current",
            "80",
        );
        apply(
            &mut s,
            "iakatokencounter/all/ia/codex/default/quota/30d/remaining_pct/current",
            "15",
        );
        let w = s.worst().expect("un pire reservoir existe");
        assert_eq!(w.remaining_pct, 15.0);
        assert_eq!(w.label, "codex default 30d");
    }

    #[test]
    fn snapshot_serialise_en_camel_case() {
        let mut s = ReservoirStore::new();
        apply(
            &mut s,
            "iakatokencounter/all/ia/claude/max/quota/5h/remaining_pct/current",
            "87.5",
        );
        let cards = s.cards();
        let json = serde_json::to_string(&cards[0]).unwrap();
        assert!(json.contains("\"fiveH\""), "camelCase attendu: {json}");
        assert!(json.contains("\"remainingPct\":87.5"), "{json}");
    }

    #[test]
    fn republication_meme_valeur_ne_change_rien() {
        let mut s = ReservoirStore::new();
        assert!(apply(
            &mut s,
            "iakatokencounter/all/ia/claude/max/quota/5h/used_pct/current",
            "12.5",
        ));
        // Meme valeur, meme t : rien ne change.
        assert!(!apply(
            &mut s,
            "iakatokencounter/all/ia/claude/max/quota/5h/used_pct/current",
            "12.5",
        ));
    }
}
