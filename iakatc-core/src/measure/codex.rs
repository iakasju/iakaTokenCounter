//! measure::codex — conso Codex CLI depuis les rollouts JSONL de session.
//!
//! Complete ce que `IakaCockpit/src-tauri/src/codex.rs @ d2058d` **ignore** explicitement :
//! l'evenement porteur d'usage `{"type":"event_msg","payload":{"type":"token_count", ...}}`.
//! On scanne les rollouts (`~/.codex/sessions/**/*.jsonl`, `CODEX_HOME` respecte) et on agrege
//! les tokens depuis ces evenements. Le `cwd` du projet est lu dans le `session_meta` en tete de
//! rollout (esprit de `rollout_cwd` cote codex.rs). LECTURE SEULE, parse **defensif**.
//!
//! ## Shape reelle confirmee (rollout Codex 0.142.3, capture 2026-06-27/29)
//! ```json
//! {"type":"event_msg","payload":{"type":"token_count",
//!   "info":{
//!     "total_token_usage":{"input_tokens":12152,"cached_input_tokens":1920,
//!                          "output_tokens":5,"reasoning_output_tokens":0,"total_tokens":12157},
//!     "last_token_usage":{...},"model_context_window":258400},
//!   "rate_limits":{"primary":{"used_percent":5.0,"window_minutes":43200,"resets_at":1785158548},
//!                  "secondary":null,"plan_type":"free",...}}}
//! ```
//! - `total_token_usage` est **cumulatif sur la session** (il croit d'un `token_count` au suivant)
//!   -> on retient le **maximum** de `total_tokens` sur la session (pas la somme des evenements).
//! - `input_tokens` **inclut** deja `cached_input_tokens` (12152 input dont 1920 caches ;
//!   `total_tokens` = input + output = 12152 + 5 = 12157). On mappe donc :
//!   `input_tokens` (contrat) = `input_tokens`, `cache_tokens` = `cached_input_tokens`,
//!   `output_tokens` = `output_tokens`, `used_tokens` = input + output.
//! - `rate_limits.primary.window_minutes` du plan free = **43200 min (30 jours)** : ne correspond
//!   ni a la fenetre 5h ni a 7d du contrat -> voir [`CodexRateLimit`] / D3 (best-effort, § open).

use super::{Agent, Measurement, Provider, Tokens};
use crate::measure::claude::project_of;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Sous-dossier (relatif au HOME) ou Codex ecrit ses rollouts, si `CODEX_HOME` n'est pas pose.
const CODEX_SESSIONS_SUBDIR: &str = ".codex/sessions";

/// Racine des sessions Codex : `$CODEX_HOME/sessions` si `CODEX_HOME` est pose, sinon
/// `<home>/.codex/sessions`. `None` si aucun home determinable (la decouverte ne trouvera rien).
pub fn codex_sessions_dir() -> Option<PathBuf> {
    if let Some(codex_home) = std::env::var_os("CODEX_HOME") {
        return Some(Path::new(&codex_home).join("sessions"));
    }
    dirs::home_dir().map(|h| h.join(CODEX_SESSIONS_SUBDIR))
}

/// Rate-limit best-effort porte par un `token_count` (D3). `window_minutes` sert a decider a
/// quelle fenetre du contrat (5h/7d) le rattacher — cf. `quota::merge`. Sur le plan free reel,
/// `window_minutes = 43200` (30 j) : ne mappe sur AUCUNE fenetre du contrat -> non publie (open).
#[derive(Debug, Clone, PartialEq)]
pub struct CodexRateLimit {
    pub used_percent: f64,
    pub window_minutes: u64,
    pub resets_at: Option<i64>,
}

/// Liste recursivement les `*.jsonl` sous `root` (arbo Codex `YYYY/MM/DD/`). Best-effort :
/// un dossier illisible est saute (jamais d'erreur propagee). Calque `codex.rs::walk_jsonl`.
fn walk_jsonl(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                out.push(path);
            }
        }
    }
    out
}

/// Lit le `session_meta.cwd` d'un rollout (toujours en TETE de fichier). Calque
/// `codex.rs::rollout_cwd` : on s'arrete au premier record JSON (le `session_meta`).
fn rollout_cwd(content: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v = serde_json::from_str::<Value>(line).ok()?;
        if v.get("type").and_then(Value::as_str) == Some("session_meta") {
            return v
                .get("payload")
                .and_then(|p| p.get("cwd"))
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        return None; // session_meta est cense etre le tout premier record.
    }
    None
}

/// Extrait les tokens cumules d'un objet `info.total_token_usage`. Defensif : champ absent -> 0.
fn tokens_from_usage(usage: &Value) -> Tokens {
    let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    Tokens {
        // `input_tokens` inclut deja les caches cote Codex (cf. entete).
        input: n("input_tokens"),
        output: n("output_tokens"),
        cache: n("cached_input_tokens"),
    }
}

/// Si `line` est un `event_msg`/`token_count`, renvoie `(Tokens cumules, rate-limits eventuels)`.
/// `None` sinon. PUR/testable, defensif (toute forme inattendue -> `None` ou champs a 0).
pub fn parse_token_count(line: &str) -> Option<(Tokens, Vec<CodexRateLimit>)> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(Value::as_str) != Some("event_msg") {
        return None;
    }
    let payload = v.get("payload")?;
    if payload.get("type").and_then(Value::as_str) != Some("token_count") {
        return None;
    }
    let usage = payload
        .get("info")
        .and_then(|i| i.get("total_token_usage"))?;
    let tokens = tokens_from_usage(usage);
    let rate_limits = parse_rate_limits(payload.get("rate_limits"));
    Some((tokens, rate_limits))
}

/// Extrait les rate-limits (`primary` + `secondary`) d'un objet `payload.rate_limits`.
/// Defensif : objet/champ absent -> liste vide ; une fenetre sans `used_percent` est ignoree.
fn parse_rate_limits(rl: Option<&Value>) -> Vec<CodexRateLimit> {
    let rl = match rl {
        Some(rl) if rl.is_object() => rl,
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    for key in ["primary", "secondary"] {
        if let Some(w) = rl.get(key).filter(|w| w.is_object()) {
            if let Some(used) = w.get("used_percent").and_then(Value::as_f64) {
                out.push(CodexRateLimit {
                    used_percent: used,
                    window_minutes: w.get("window_minutes").and_then(Value::as_u64).unwrap_or(0),
                    resets_at: w.get("resets_at").and_then(Value::as_i64),
                });
            }
        }
    }
    out
}

/// Agrege UN rollout (contenu complet) : cwd -> projet, et **maximum** de `total_token_usage`
/// (cumulatif). Renvoie `None` si pas de projet ou aucun `token_count` non nul.
fn fold_rollout(content: &str) -> Option<(String, Tokens, Vec<CodexRateLimit>)> {
    let project = rollout_cwd(content).and_then(|c| project_of(&c))?;
    let mut best = Tokens::default();
    let mut last_rl: Vec<CodexRateLimit> = Vec::new();
    for line in content.lines() {
        if let Some((tokens, rl)) = parse_token_count(line) {
            // Cumulatif : on garde l'evenement au total le plus eleve.
            if tokens.used() >= best.used() {
                best = tokens;
            }
            if !rl.is_empty() {
                last_rl = rl;
            }
        }
    }
    if best.used() == 0 {
        return None;
    }
    Some((project, best, last_rl))
}

/// Scanne la racine des sessions Codex : renvoie les mesures par projet (agent = coordinator,
/// Codex n'a pas de sidechain) ET les rate-limits best-effort du rollout le plus « riche »
/// rencontre (plus grand `used_tokens` — proxy du plus recent, faute de tri par mtime ici).
/// Defensif : fichier illisible ignore, jamais de panique.
pub fn scan_codex(sessions_root: &Path) -> (Vec<Measurement>, Vec<CodexRateLimit>) {
    let mut acc: std::collections::HashMap<String, Tokens> = std::collections::HashMap::new();
    let mut best_rl: Vec<CodexRateLimit> = Vec::new();
    let mut best_rl_used = 0u64;
    for path in walk_jsonl(sessions_root) {
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if let Some((project, tokens, rl)) = fold_rollout(&content) {
            if !rl.is_empty() && tokens.used() >= best_rl_used {
                best_rl_used = tokens.used();
                best_rl = rl;
            }
            acc.entry(project).or_default().add(&tokens);
        }
    }
    let mut out: Vec<Measurement> = acc
        .into_iter()
        .map(|(project, tokens)| Measurement {
            project,
            provider: Provider::Codex,
            agent: Agent::Coordinator,
            tokens,
        })
        .collect();
    out.sort_by(|a, b| a.project.cmp(&b.project));
    (out, best_rl)
}

/// Variante ne renvoyant que les mesures (le rate-limit Codex etant best-effort).
pub fn scan_codex_measurements(sessions_root: &Path) -> Vec<Measurement> {
    scan_codex(sessions_root).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_token_count_lit_la_shape_reelle() {
        let line = r#"{"timestamp":"2026-06-27T13:22:29.958Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":12152,"cached_input_tokens":1920,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":12157},"last_token_usage":{"input_tokens":12152,"cached_input_tokens":1920,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":12157},"model_context_window":258400},"rate_limits":{"limit_id":"codex","limit_name":null,"primary":{"used_percent":5.0,"window_minutes":43200,"resets_at":1785158548},"secondary":null,"credits":null,"individual_limit":null,"plan_type":"free","rate_limit_reached_type":null}}}"#;
        let (tokens, rl) = parse_token_count(line).unwrap();
        assert_eq!(tokens.input, 12152);
        assert_eq!(tokens.output, 5);
        assert_eq!(tokens.cache, 1920);
        assert_eq!(tokens.used(), 12157); // input + output = total_tokens
        assert_eq!(rl.len(), 1); // primary seul (secondary null)
        assert_eq!(rl[0].used_percent, 5.0);
        assert_eq!(rl[0].window_minutes, 43200); // 30 jours (plan free)
        assert_eq!(rl[0].resets_at, Some(1785158548));
    }

    #[test]
    fn parse_token_count_ignore_les_autres_evenements() {
        // event_msg non-token_count, response_item, session_meta, non-JSON : tous None.
        for raw in [
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"OK"}}"#,
            r#"{"type":"response_item","payload":{"type":"reasoning","summary":"x"}}"#,
            r#"{"type":"session_meta","payload":{"cwd":"/p"}}"#,
            r#"{"type":"event_msg","payload":{"type":"token_count"}}"#, // pas d'info -> None
            "pas du json {",
            "",
        ] {
            assert!(parse_token_count(raw).is_none(), "doit etre None : {raw}");
        }
    }

    #[test]
    fn parse_token_count_defensif_champs_manquants() {
        // info.total_token_usage present mais champs partiels : pas de panique, 0 par defaut.
        let line = r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":42}}}}"#;
        let (tokens, rl) = parse_token_count(line).unwrap();
        assert_eq!(tokens.input, 42);
        assert_eq!(tokens.output, 0);
        assert_eq!(tokens.cache, 0);
        assert!(rl.is_empty()); // pas de rate_limits
    }

    #[test]
    fn fold_rollout_retient_le_max_cumulatif() {
        // total_token_usage croit (cumulatif) : on garde le plus grand, pas la somme.
        let content = concat!(
            r#"{"type":"session_meta","payload":{"cwd":"/w/proj-codex"}}"#,
            "\n",
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":12166,"cached_input_tokens":1920,"output_tokens":176}}}}"#,
            "\n",
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":24607,"cached_input_tokens":6912,"output_tokens":201}}}}"#,
            "\n"
        );
        let (project, tokens, _rl) = fold_rollout(content).unwrap();
        assert_eq!(project, "proj-codex");
        assert_eq!(tokens.input, 24607); // max cumulatif, PAS 12166+24607
        assert_eq!(tokens.output, 201);
        assert_eq!(tokens.used(), 24808);
    }

    #[test]
    fn fold_rollout_none_sans_projet_ou_sans_usage() {
        // Pas de session_meta -> pas de projet.
        assert!(fold_rollout(r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10}}}}"#).is_none());
        // session_meta mais aucun token_count non nul.
        let c = concat!(
            r#"{"type":"session_meta","payload":{"cwd":"/w/p"}}"#,
            "\n",
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"OK"}}"#
        );
        assert!(fold_rollout(c).is_none());
    }

    #[test]
    fn fixture_reelle_produit_une_mesure_codex_non_nulle() {
        // Rollout REEL capture (session_meta + token_count) — cf. specs/mock.
        let raw = include_str!("../../../specs/mock/codex_rollout_sample.jsonl");
        let (project, tokens, rl) = fold_rollout(raw).expect("la fixture doit produire une mesure");
        assert_eq!(project, "codex-recette");
        assert!(tokens.used() > 0, "used_tokens Codex doit etre > 0");
        assert!(!rl.is_empty(), "la fixture reelle porte des rate_limits");
    }
}
