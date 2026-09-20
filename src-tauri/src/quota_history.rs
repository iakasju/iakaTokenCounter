//! quota_history — echantillonnage continu du quota (5 fenetres/compte) + persistance JSONL bornee
//! a 90 jours.
//!
//! **Decalque direct de `memory.rs`** (memes fonctions pures I/O `append_sample` / `read_history` /
//! `compact`, meme patron de thread detache qui ne panique jamais, meme resolution de chemin sous
//! `app_data_dir`). Ce que `memory.rs` fait pour la RAM du poste, ce module le fait pour le quota
//! IA : le broker MQTT ne retient qu'une valeur COURANTE (retained, pas de serie), et un
//! redemarrage du broker repart d'un etat vide. Le quota est deja recu et decode par
//! `state::ReservoirStore` (`AppState::snapshot()`) : ce module n'ouvre AUCUN abonnement, il lit
//! l'etat deja agrege et l'appende.
//!
//! Pourquoi persister ICI (D1 `specs/instructions/feature-memoire-historique.md`) : « Vais-je tenir
//! jusqu'au rechargement ? » n'a aucune reponse possible avec les sources actuelles (statusline ->
//! MQTT retained -> valeur courante seulement).
//!
//! Un point par `(provider, account, window)`, ecrit **toutes les 5 min si la valeur a change**, ou
//! si **plus d'une heure** s'est ecoulee depuis le dernier point de cette serie (point horaire
//! force, garde les plateaux visibles). Retention glissante : **90 jours** (trois cycles de la plus
//! longue fenetre du contrat).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager};

use crate::memory::now_secs;
use crate::state::{AppState, Window};

/// Nom du fichier d'historique de quota, joint au repertoire de donnees de l'app.
pub const HISTORY_FILE: &str = "quota-history.jsonl";

/// Cadence d'echantillonnage : lecture de l'etat quota toutes les 5 min (D2).
const SAMPLE_INTERVAL_SECS: u64 = 300;

/// Point horaire force (D2) : au-dela, un point est ecrit meme si la valeur n'a pas change.
const FORCE_INTERVAL_SECS: i64 = 3_600;

/// Compaction toutes les N ecritures. Avec au plus 5 reservoirs x 3 fenetres = 15 series et un
/// tick toutes les 5 min, 500 ecritures represente au pire quelques heures d'activite continue.
const COMPACT_EVERY: u64 = 500;

/// Retention glissante du quota : 90 jours (D2 — trois cycles de la plus longue fenetre du
/// contrat, la fenetre 30 j du plan free Codex).
pub const RETENTION_SECS: i64 = 90 * 86_400;

/// Un point de quota persiste pour une serie `(provider, account, window)`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSample {
    /// Epoch en secondes.
    pub t: i64,
    pub provider: String,
    pub account: String,
    /// Code de fenetre du contrat (`5h` / `7d` / `30d`).
    pub window: String,
    pub used_pct: Option<f64>,
    pub remaining_pct: Option<f64>,
    pub resets_at: Option<i64>,
    pub confidence: Option<String>,
}

/// Representation disque compacte (cles courtes pour limiter la taille du JSONL, patron `memory.rs`).
#[derive(Debug, Serialize, Deserialize)]
struct Line {
    t: i64,
    p: String,
    a: String,
    w: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    u: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    r: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    rs: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    c: Option<String>,
}

impl From<&QuotaSample> for Line {
    fn from(s: &QuotaSample) -> Self {
        Line {
            t: s.t,
            p: s.provider.clone(),
            a: s.account.clone(),
            w: s.window.clone(),
            u: s.used_pct,
            r: s.remaining_pct,
            rs: s.resets_at,
            c: s.confidence.clone(),
        }
    }
}

impl From<Line> for QuotaSample {
    fn from(l: Line) -> Self {
        QuotaSample {
            t: l.t,
            provider: l.p,
            account: l.a,
            window: l.w,
            used_pct: l.u,
            remaining_pct: l.r,
            resets_at: l.rs,
            confidence: l.c,
        }
    }
}

/// Cle d'une serie de quota : `(provider, account, window)`.
type SeriesKey = (String, String, String);

fn series_key(s: &QuotaSample) -> SeriesKey {
    (s.provider.clone(), s.account.clone(), s.window.clone())
}

/// Append un echantillon en fin de fichier JSONL. Cree le dossier parent et le fichier au besoin.
pub fn append_sample(path: &Path, sample: &QuotaSample) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(&Line::from(sample)).map_err(std::io::Error::other)?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{line}")?;
    Ok(())
}

/// Lit l'historique persistant, **trie par `t` croissant**. Fichier absent -> `Vec` vide (defensif).
/// Les lignes illisibles (corrompues) sont **ignorees** sans planter.
pub fn read_history(path: &Path) -> Vec<QuotaSample> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut out: Vec<QuotaSample> = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str::<Line>(&l).ok())
        .map(QuotaSample::from)
        .collect();
    out.sort_by_key(|s| s.t);
    out
}

/// Compaction : reecrit le fichier en ne gardant que les points de la fenetre de retention
/// (`t >= now - retention_secs`). Ecriture **atomique** (`.tmp` filtre puis `rename`). Fichier
/// absent -> no-op.
pub fn compact(path: &Path, retention_secs: i64, now: i64) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let cutoff = now - retention_secs;
    let kept: Vec<QuotaSample> = read_history(path)
        .into_iter()
        .filter(|s| s.t >= cutoff)
        .collect();
    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        for s in &kept {
            let line = serde_json::to_string(&Line::from(s)).map_err(std::io::Error::other)?;
            writeln!(f, "{line}")?;
        }
        f.flush()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Decide si un nouveau point doit etre ecrit pour une serie (D2) : valeur changee depuis le
/// dernier point connu, ou plus d'une heure ecoulee depuis ce dernier point (point horaire force).
/// PUR/testable.
fn should_write(last: Option<&QuotaSample>, candidate: &QuotaSample) -> bool {
    match last {
        None => true,
        Some(last) => {
            let changed = last.used_pct != candidate.used_pct
                || last.remaining_pct != candidate.remaining_pct
                || last.resets_at != candidate.resets_at
                || last.confidence != candidate.confidence;
            let stale = candidate.t - last.t > FORCE_INTERVAL_SECS;
            changed || stale
        }
    }
}

/// Etat du log de quota porte par `AppState` : chemin resolu du fichier JSONL persistant. Le
/// `Mutex` qui l'enveloppe **serialise** les acces entre le thread sampler (ecrit) et la commande
/// (lit) — meme patron que `memory::MemoryLog`.
#[derive(Debug, Default)]
pub struct QuotaLog {
    pub path: PathBuf,
}

impl QuotaLog {
    /// Construit le log a partir du repertoire de donnees de l'app (joint le nom de fichier).
    pub fn in_dir(dir: &Path) -> Self {
        QuotaLog { path: dir.join(HISTORY_FILE) }
    }
}

/// Lance le sampler de quota dans un **thread detache** (patron `memory::start_sampler`). Ne
/// bloque pas ; **ne panique jamais** (toute erreur I/O est loggee et la boucle continue).
pub fn start_sampler(app: AppHandle) {
    std::thread::spawn(move || run_sampler(app));
}

fn run_sampler(app: AppHandle) {
    let path = {
        let state = app.state::<AppState>();
        let log = state.quota_history.lock().unwrap();
        log.path.clone()
    };
    // Compaction au demarrage : borne le fichier des le lancement.
    {
        let state = app.state::<AppState>();
        let _guard = state.quota_history.lock().unwrap();
        if let Err(e) = compact(&path, RETENTION_SECS, now_secs()) {
            eprintln!("[iakatc-tray] compaction quota initiale echouee: {e}");
        }
    }
    // Dernier point ecrit par serie : seede depuis l'historique existant (redemarrage), pour que
    // la regle "changee ou > 1h" reste correcte a travers un redemarrage du tray.
    let mut last: HashMap<SeriesKey, QuotaSample> = HashMap::new();
    for s in read_history(&path) {
        last.insert(series_key(&s), s);
    }

    let mut writes: u64 = 0;
    loop {
        let now = now_secs();
        let snapshot = app.state::<AppState>().snapshot();
        for card in &snapshot.reservoirs {
            for (window, ws) in [
                (Window::FiveHour, &card.five_h),
                (Window::SevenDay, &card.seven_d),
                (Window::ThirtyDay, &card.thirty_d),
            ] {
                // Fenetre sans aucune donnee connue (ex. thirty_d hors Codex) : rien a persister.
                if ws.used_pct.is_none()
                    && ws.remaining_pct.is_none()
                    && ws.resets_at.is_none()
                    && ws.confidence.is_none()
                {
                    continue;
                }
                let candidate = QuotaSample {
                    t: now,
                    provider: card.provider.clone(),
                    account: card.account.clone(),
                    window: window.code().to_string(),
                    used_pct: ws.used_pct,
                    remaining_pct: ws.remaining_pct,
                    resets_at: ws.resets_at,
                    confidence: ws.confidence.clone(),
                };
                let key = series_key(&candidate);
                if should_write(last.get(&key), &candidate) {
                    let state = app.state::<AppState>();
                    let _guard = state.quota_history.lock().unwrap();
                    if let Err(e) = append_sample(&path, &candidate) {
                        eprintln!("[iakatc-tray] append echantillon quota echoue: {e}");
                    } else {
                        writes = writes.wrapping_add(1);
                        if writes.is_multiple_of(COMPACT_EVERY) {
                            if let Err(e) = compact(&path, RETENTION_SECS, now_secs()) {
                                eprintln!("[iakatc-tray] compaction quota echouee: {e}");
                            }
                        }
                    }
                    last.insert(key, candidate);
                }
            }
        }
        std::thread::sleep(Duration::from_secs(SAMPLE_INTERVAL_SECS));
    }
}

/// Commande : historique de quota persistant (fenetre de retention 90 j), trie par `t` croissant.
/// Fichier absent -> serie vide (defensif, jamais d'erreur).
#[tauri::command]
pub fn get_quota_history(state: tauri::State<'_, AppState>) -> Vec<QuotaSample> {
    let log = state.quota_history.lock().unwrap();
    read_history(&log.path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp_history(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("itc-quota-{name}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(HISTORY_FILE)
    }

    /// `resets_at` fixe (n'avance pas avec `t`) : en usage reel, c'est l'echeance de la fenetre de
    /// quota, qui ne bouge PAS a chaque echantillon — seulement quand la fenetre elle-meme se
    /// recharge. Le lier a `t` ferait paraitre chaque nouvel echantillon "change" a tort et
    /// fausserait les tests de `should_write` (constate : deux echantillons de meme valeur a des
    /// `t` differents doivent etre vus comme inchanges).
    const FIXED_RESETS_AT: i64 = 999_999;

    fn sample(t: i64, provider: &str, account: &str, window: &str, used_pct: f64) -> QuotaSample {
        QuotaSample {
            t,
            provider: provider.to_string(),
            account: account.to_string(),
            window: window.to_string(),
            used_pct: Some(used_pct),
            remaining_pct: Some(100.0 - used_pct),
            resets_at: Some(FIXED_RESETS_AT),
            confidence: Some("official".to_string()),
        }
    }

    #[test]
    fn round_trip_append_read_trie() {
        let path = tmp_history("rt");
        append_sample(&path, &sample(30, "claude", "max", "5h", 10.0)).unwrap();
        append_sample(&path, &sample(10, "claude", "max", "5h", 5.0)).unwrap();
        append_sample(&path, &sample(20, "claude", "max", "5h", 8.0)).unwrap();
        let h = read_history(&path);
        assert_eq!(h.iter().map(|s| s.t).collect::<Vec<_>>(), vec![10, 20, 30]);
        assert_eq!(h[0].used_pct, Some(5.0));
    }

    #[test]
    fn read_history_fichier_absent_vide() {
        let path = std::env::temp_dir().join("itc-quota-absent-xyz/nope.jsonl");
        assert!(read_history(&path).is_empty());
    }

    #[test]
    fn ligne_corrompue_ignoree() {
        let path = tmp_history("corrupt");
        append_sample(&path, &sample(10, "claude", "max", "5h", 5.0)).unwrap();
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            writeln!(f, "pas du json valide").unwrap();
        }
        append_sample(&path, &sample(20, "claude", "max", "5h", 6.0)).unwrap();
        let h = read_history(&path);
        assert_eq!(h.iter().map(|s| s.t).collect::<Vec<_>>(), vec![10, 20]);
    }

    #[test]
    fn compact_retire_les_vieux_points_garde_les_recents() {
        let path = tmp_history("compact");
        let now = 100_000_000;
        append_sample(&path, &sample(now - 90 * 86_400 - 1, "claude", "max", "5h", 1.0)).unwrap();
        append_sample(&path, &sample(now - 100, "claude", "max", "5h", 2.0)).unwrap();
        append_sample(&path, &sample(now, "claude", "max", "5h", 3.0)).unwrap();
        compact(&path, RETENTION_SECS, now).unwrap();
        let h = read_history(&path);
        assert_eq!(h.len(), 2);
        assert!(h.iter().all(|s| s.t >= now - RETENTION_SECS));
    }

    #[test]
    fn compact_fichier_absent_noop() {
        let path = std::env::temp_dir().join("itc-quota-absent-compact-zzz/none.jsonl");
        compact(&path, RETENTION_SECS, 1_000_000).unwrap();
    }

    #[test]
    fn should_write_premier_point_toujours_ecrit() {
        assert!(should_write(None, &sample(10, "claude", "max", "5h", 5.0)));
    }

    #[test]
    fn should_write_valeur_inchangee_ne_reecrit_pas() {
        let last = sample(0, "claude", "max", "5h", 5.0);
        let candidate = sample(300, "claude", "max", "5h", 5.0); // 5 min plus tard, meme valeur
        assert!(!should_write(Some(&last), &candidate));
    }

    #[test]
    fn should_write_valeur_changee_reecrit() {
        let last = sample(0, "claude", "max", "5h", 5.0);
        let candidate = sample(300, "claude", "max", "5h", 7.0); // valeur differente
        assert!(should_write(Some(&last), &candidate));
    }

    #[test]
    fn should_write_point_horaire_force_meme_valeur_inchangee() {
        let last = sample(0, "claude", "max", "5h", 5.0);
        let candidate = sample(3_601, "claude", "max", "5h", 5.0); // > 1h, meme valeur
        assert!(should_write(Some(&last), &candidate));
        let candidate_avant = sample(3_600, "claude", "max", "5h", 5.0); // pile 1h : pas encore force
        assert!(!should_write(Some(&last), &candidate_avant));
    }

    #[test]
    fn sample_serialise_en_camel_case() {
        let json = serde_json::to_string(&sample(5, "claude", "max", "5h", 12.5)).unwrap();
        assert!(json.contains("\"usedPct\":12.5"), "{json}");
        assert!(json.contains("\"remainingPct\":87.5"), "{json}");
        assert!(json.contains("\"resetsAt\""), "{json}");
    }

    #[test]
    fn ligne_disque_utilise_des_cles_courtes() {
        let line = serde_json::to_string(&Line::from(&sample(5, "claude", "max", "5h", 12.5))).unwrap();
        assert!(line.contains("\"u\":12.5"), "{line}");
        assert!(line.contains("\"w\":\"5h\""), "{line}");
    }

    #[test]
    fn series_key_distingue_provider_account_window() {
        let a = sample(0, "claude", "max", "5h", 1.0);
        let b = sample(0, "claude", "max", "7d", 1.0);
        let c = sample(0, "codex", "max", "5h", 1.0);
        assert_ne!(series_key(&a), series_key(&b));
        assert_ne!(series_key(&a), series_key(&c));
    }
}
