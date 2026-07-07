//! quota::merge — fusion **hybride** du quota (D5) : par `(provider, account, window)`, choisir
//! UNE valeur et **etiqueter** la confiance selon 4 branches :
//!
//! 1. **official**       : fichier quota present, fenetre presente, `resets_at > now`, capture
//!    plus recente que le seuil de fraicheur.
//! 2. **official_stale** : idem mais capture au-dela du seuil.
//! 3. **local_estimate** : sinon, si plafond configure + tokens mesures -> `tokens / plafond`.
//! 4. **none**           : sinon ; `used_pct = null`, mais `used_tokens` reste remonte (diagnostic).
//!
//! Le `Reservoir` est ensuite **decompose en codes scalaires** par `publish::contract` (aucun objet
//! composite sur le fil). Codex : quota **best-effort** via [`codex_reservoirs`] (source
//! `codex_rollout`) quand une fenetre `token_count` mappe sur 5h/7d.

use super::config::Config;
use super::store::QuotaFile;
use crate::measure::codex::CodexRateLimit;
use std::collections::{BTreeSet, HashMap};

/// Fenetre de quota du contrat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Window {
    FiveHour,
    SevenDay,
}

impl Window {
    /// Code de fenetre publie dans les topics (`5h` / `7d`).
    pub fn code(self) -> &'static str {
        match self {
            Window::FiveHour => "5h",
            Window::SevenDay => "7d",
        }
    }

    /// Les deux fenetres, dans l'ordre stable de publication.
    pub fn all() -> [Window; 2] {
        [Window::FiveHour, Window::SevenDay]
    }
}

/// Niveau de confiance de la valeur retenue (§ 3.2 du contrat).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    Official,
    OfficialStale,
    LocalEstimate,
    None,
}

impl Confidence {
    pub fn code(self) -> &'static str {
        match self {
            Confidence::Official => "official",
            Confidence::OfficialStale => "official_stale",
            Confidence::LocalEstimate => "local_estimate",
            Confidence::None => "none",
        }
    }
}

/// Provenance de la valeur retenue (§ 3.2 du contrat).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Statusline,
    JsonlEstimate,
    CodexRollout,
    Config,
}

impl Source {
    pub fn code(self) -> &'static str {
        match self {
            Source::Statusline => "statusline",
            Source::JsonlEstimate => "jsonl_estimate",
            Source::CodexRollout => "codex_rollout",
            Source::Config => "config",
        }
    }
}

/// Etat fusionne d'un quota pour un `(provider, account, window)` — le « Reservoir ».
#[derive(Debug, Clone, PartialEq)]
pub struct Reservoir {
    pub provider: String,
    pub account: String,
    pub window: Window,
    pub used_pct: Option<f64>,
    pub remaining_pct: Option<f64>,
    pub used_tokens: Option<u64>,
    pub resets_at: Option<i64>,
    pub captured_at: Option<i64>,
    pub confidence: Confidence,
    pub source: Option<Source>,
}

/// Seuil de fraicheur applicable a une fenetre, depuis la config.
fn freshness_seconds(config: &Config, window: Window) -> i64 {
    match window {
        Window::FiveHour => config.freshness.five_hour_seconds,
        Window::SevenDay => config.freshness.seven_day_seconds,
    }
}

/// Plafond configure pour une fenetre (`None` si non configure).
fn ceiling_tokens(config: &Config, provider: &str, account: &str, window: Window) -> Option<u64> {
    let c = config.ceiling(provider, account);
    match window {
        Window::FiveHour => c.five_hour_tokens,
        Window::SevenDay => c.seven_day_tokens,
    }
}

/// Fenetre exacte (statusline) d'un fichier quota pour une fenetre donnee.
fn window_quota(file: &QuotaFile, window: Window) -> Option<&super::store::WindowQuota> {
    match window {
        Window::FiveHour => file.rate_limits.five_hour.as_ref(),
        Window::SevenDay => file.rate_limits.seven_day.as_ref(),
    }
}

/// Fusionne UN `(provider, account, window)`. `exact` = fichier quota du couple (s'il existe),
/// `measured` = tokens mesures du provider (diagnostic), `now` = epoch s courant.
fn merge_one(
    provider: &str,
    account: &str,
    window: Window,
    exact: Option<&QuotaFile>,
    config: &Config,
    measured: Option<u64>,
    now: i64,
) -> Reservoir {
    // --- Branches 1/2 : exact utilisable (fenetre presente ET non rechargee). ---
    if let Some(file) = exact {
        if let Some(wq) = window_quota(file, window) {
            if wq.resets_at > now {
                let fresh = now - file.captured_at <= freshness_seconds(config, window);
                let confidence = if fresh {
                    Confidence::Official
                } else {
                    Confidence::OfficialStale
                };
                let used = wq.used_percentage;
                return Reservoir {
                    provider: provider.to_string(),
                    account: account.to_string(),
                    window,
                    used_pct: Some(used),
                    remaining_pct: Some(100.0 - used),
                    used_tokens: measured,
                    resets_at: Some(wq.resets_at),
                    captured_at: Some(file.captured_at),
                    confidence,
                    source: Some(Source::Statusline),
                };
            }
        }
    }

    // --- Branche 3 : estimation locale (plafond configure + tokens mesures). ---
    if let (Some(ceiling), Some(tokens)) = (
        ceiling_tokens(config, provider, account, window),
        measured,
    ) {
        if ceiling > 0 {
            let used = ((tokens as f64) / (ceiling as f64) * 100.0).min(100.0);
            return Reservoir {
                provider: provider.to_string(),
                account: account.to_string(),
                window,
                used_pct: Some(used),
                remaining_pct: Some(100.0 - used),
                used_tokens: Some(tokens),
                resets_at: None,
                captured_at: None,
                confidence: Confidence::LocalEstimate,
                source: Some(Source::JsonlEstimate),
            };
        }
    }

    // --- Branche 4 : aucune valeur exploitable (used_pct null, used_tokens diagnostic). ---
    Reservoir {
        provider: provider.to_string(),
        account: account.to_string(),
        window,
        used_pct: None,
        remaining_pct: None,
        used_tokens: measured,
        resets_at: None,
        captured_at: None,
        confidence: Confidence::None,
        source: None,
    }
}

/// Fusion complete : produit un `Reservoir` par `(provider, account, window)`.
///
/// Les couples `(provider, account)` proviennent des fichiers quota **et** des providers mesures
/// sans fichier (couple `(provider, "default")`) — pour toujours remonter le `used_tokens`
/// diagnostic meme sans statusline. `measured_by_provider` : code provider -> total used_tokens.
pub fn merge(
    quota_files: &[QuotaFile],
    config: &Config,
    measured_by_provider: &HashMap<String, u64>,
    now: i64,
) -> Vec<Reservoir> {
    // Ensemble des couples (provider, account) a produire.
    let mut pairs: BTreeSet<(String, String)> = BTreeSet::new();
    for f in quota_files {
        pairs.insert((f.provider.clone(), f.account.clone()));
    }
    let covered_providers: BTreeSet<&str> = quota_files.iter().map(|f| f.provider.as_str()).collect();
    for provider in measured_by_provider.keys() {
        if !covered_providers.contains(provider.as_str()) {
            pairs.insert((provider.clone(), "default".to_string()));
        }
    }

    let mut out = Vec::new();
    for (provider, account) in &pairs {
        let exact = quota_files
            .iter()
            .find(|f| &f.provider == provider && &f.account == account);
        let measured = measured_by_provider.get(provider).copied();
        for window in Window::all() {
            out.push(merge_one(
                provider, account, window, exact, config, measured, now,
            ));
        }
    }
    out
}

/// Mappe une `window_minutes` Codex sur une fenetre du contrat, avec tolerance. Renvoie `None`
/// si la fenetre ne correspond ni a 5h ni a 7d (cas du plan free : 43200 min = 30 j -> `None`).
pub fn codex_window(window_minutes: u64) -> Option<Window> {
    match window_minutes {
        240..=360 => Some(Window::FiveHour),   // ~5 h (300 min +/- tolerance)
        8640..=11520 => Some(Window::SevenDay), // ~7 j (10080 min +/- tolerance)
        _ => None,
    }
}

/// Quota Codex **best-effort** (D3) : convertit les rate-limits d'un `token_count` en `Reservoir`
/// (source `codex_rollout`, confiance `official`) pour les seules fenetres qui mappent sur 5h/7d.
/// `measured` = used_tokens Codex mesures (diagnostic). Le plan free (30 j) ne produit rien.
pub fn codex_reservoirs(
    account: &str,
    rate_limits: &[CodexRateLimit],
    measured: Option<u64>,
    _now: i64,
) -> Vec<Reservoir> {
    let mut out = Vec::new();
    for rl in rate_limits {
        if let Some(window) = codex_window(rl.window_minutes) {
            let used = rl.used_percent;
            out.push(Reservoir {
                provider: "codex".to_string(),
                account: account.to_string(),
                window,
                used_pct: Some(used),
                remaining_pct: Some(100.0 - used),
                used_tokens: measured,
                resets_at: rl.resets_at,
                captured_at: None,
                confidence: Confidence::Official,
                source: Some(Source::CodexRollout),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::store::{RateLimits, WindowQuota};

    fn qfile(provider: &str, account: &str, captured_at: i64, five_h: Option<(f64, i64)>) -> QuotaFile {
        QuotaFile {
            account: account.into(),
            provider: provider.into(),
            captured_at,
            source_version: None,
            rate_limits: RateLimits {
                five_hour: five_h.map(|(used, resets)| WindowQuota {
                    used_percentage: used,
                    resets_at: resets,
                }),
                seven_day: None,
            },
        }
    }

    fn find<'a>(v: &'a [Reservoir], account: &str, window: Window) -> &'a Reservoir {
        v.iter()
            .find(|r| r.account == account && r.window == window)
            .expect("reservoir attendu")
    }

    #[test]
    fn branche1_exact_frais_official() {
        let now = 1000;
        let files = vec![qfile("claude", "max", now - 60, Some((23.5, now + 5000)))];
        let r = merge(&files, &Config::default(), &HashMap::new(), now);
        let five = find(&r, "max", Window::FiveHour);
        assert_eq!(five.confidence, Confidence::Official);
        assert_eq!(five.source, Some(Source::Statusline));
        assert_eq!(five.used_pct, Some(23.5));
        assert_eq!(five.remaining_pct, Some(76.5));
        assert_eq!(five.resets_at, Some(now + 5000));
    }

    #[test]
    fn branche2_exact_perime_official_stale() {
        let now = 100_000;
        // captured_at tres ancien (au-dela des 20 min) mais resets_at futur.
        let files = vec![qfile("claude", "max", now - 9999, Some((40.0, now + 5000)))];
        let r = merge(&files, &Config::default(), &HashMap::new(), now);
        let five = find(&r, "max", Window::FiveHour);
        assert_eq!(five.confidence, Confidence::OfficialStale);
        assert_eq!(five.used_pct, Some(40.0));
    }

    #[test]
    fn branche3_estimation_locale_avec_plafond() {
        let now = 1000;
        let config: Config = serde_json::from_str(
            r#"{"ceilings":{"claude":{"default":{"five_hour_tokens":1000}}}}"#,
        )
        .unwrap();
        let mut measured = HashMap::new();
        measured.insert("claude".to_string(), 250u64);
        let r = merge(&[], &config, &measured, now);
        let five = find(&r, "default", Window::FiveHour);
        assert_eq!(five.confidence, Confidence::LocalEstimate);
        assert_eq!(five.source, Some(Source::JsonlEstimate));
        assert_eq!(five.used_pct, Some(25.0)); // 250/1000*100
        assert_eq!(five.used_tokens, Some(250));
    }

    #[test]
    fn branche3_plafonne_a_100() {
        let now = 1000;
        let config: Config = serde_json::from_str(
            r#"{"ceilings":{"claude":{"default":{"five_hour_tokens":100}}}}"#,
        )
        .unwrap();
        let mut measured = HashMap::new();
        measured.insert("claude".to_string(), 999u64);
        let r = merge(&[], &config, &measured, now);
        assert_eq!(find(&r, "default", Window::FiveHour).used_pct, Some(100.0));
    }

    #[test]
    fn branche4_aucune_valeur_used_pct_null_mais_used_tokens_remonte() {
        let now = 1000;
        let mut measured = HashMap::new();
        measured.insert("claude".to_string(), 4242u64);
        // Pas de fichier quota, pas de plafond configure.
        let r = merge(&[], &Config::default(), &measured, now);
        let five = find(&r, "default", Window::FiveHour);
        assert_eq!(five.confidence, Confidence::None);
        assert_eq!(five.used_pct, None);
        assert_eq!(five.remaining_pct, None);
        assert_eq!(five.used_tokens, Some(4242)); // diagnostic conserve
        assert_eq!(five.source, None);
    }

    #[test]
    fn exact_recharge_bascule_hors_official() {
        // resets_at <= now : la fenetre est rechargee -> plus utilisable en exact -> branche none.
        let now = 100_000;
        let files = vec![qfile("claude", "max", now - 10, Some((80.0, now - 1)))];
        let r = merge(&files, &Config::default(), &HashMap::new(), now);
        assert_eq!(find(&r, "max", Window::FiveHour).confidence, Confidence::None);
    }

    #[test]
    fn codex_window_mappe_ou_none() {
        assert_eq!(codex_window(300), Some(Window::FiveHour));
        assert_eq!(codex_window(10080), Some(Window::SevenDay));
        assert_eq!(codex_window(43200), None); // plan free 30 j -> non publie
    }

    #[test]
    fn codex_reservoirs_best_effort_source_rollout() {
        let rl = vec![
            CodexRateLimit {
                used_percent: 12.0,
                window_minutes: 300,
                resets_at: Some(555),
            },
            CodexRateLimit {
                used_percent: 7.0,
                window_minutes: 43200, // 30 j -> ignore
                resets_at: Some(999),
            },
        ];
        let r = codex_reservoirs("default", &rl, Some(1234), 0);
        assert_eq!(r.len(), 1, "seule la fenetre 5h mappe");
        assert_eq!(r[0].window, Window::FiveHour);
        assert_eq!(r[0].source, Some(Source::CodexRollout));
        assert_eq!(r[0].confidence, Confidence::Official);
        assert_eq!(r[0].used_pct, Some(12.0));
        assert_eq!(r[0].used_tokens, Some(1234));
    }
}
