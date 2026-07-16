//! memory — echantillonnage continu de la RAM du poste hote + persistance JSONL bornee.
//!
//! Metrique systeme **orthogonale** au produit (conso IA / quota) : widget d'observabilite local.
//! Le sampler vit dans le process tray (thread detache, patron `mqtt_sub::start`) : il echantillonne
//! **meme fenetre fermee**, et l'historique **survit a un redemarrage** (fichier
//! `memory-history.jsonl` dans `app_data_dir`). La webview lit l'historique (`get_memory_history`)
//! et ecoute l'evenement `tray://memory` pour la croissance live (aucun polling).
//!
//! Coeur testable : les I/O disque sont des **fonctions pures** prenant le chemin en parametre
//! (`append_sample` / `read_history` / `compact`), verifiables sur un dossier temporaire. Le thread
//! **ne panique jamais** (toute erreur I/O est loggee et la boucle continue).

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::{MemoryRefreshKind, RefreshKind, System};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

/// Nom du fichier d'historique persistant, joint au repertoire de donnees de l'app.
pub const HISTORY_FILE: &str = "memory-history.jsonl";

/// Nom d'evenement pousse a la webview a chaque nouvel echantillon memoire (croissance live).
pub const MEMORY_EVENT: &str = "tray://memory";

/// Cadence d'echantillonnage : 1 point / 60 s (stockage => cadence plus lache que du live).
const SAMPLE_INTERVAL_SECS: u64 = 60;

/// Compaction (troncature de la fenetre) toutes les N ecritures (~1 h a 60 s).
const COMPACT_EVERY: u64 = 60;

/// Retention glissante de l'historique : 24 h (=> <= 1440 points a 60 s).
pub const RETENTION_SECS: i64 = 86_400;

/// Un echantillon memoire instantane. Serialise en **camelCase** pour la webview
/// (`usedBytes` / `totalBytes`) ; persiste en **cles courtes** sur disque (voir `Line`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySample {
    /// Epoch en secondes.
    pub t: i64,
    /// RAM reellement utilisee (octets, hors cache/buffers).
    pub used_bytes: u64,
    /// RAM totale du poste (octets).
    pub total_bytes: u64,
}

/// Representation disque compacte (cles courtes pour limiter la taille du JSONL).
#[derive(Debug, Serialize, Deserialize)]
struct Line {
    t: i64,
    u: u64,
    tot: u64,
}

impl From<&MemorySample> for Line {
    fn from(s: &MemorySample) -> Self {
        Line { t: s.t, u: s.used_bytes, tot: s.total_bytes }
    }
}

impl From<Line> for MemorySample {
    fn from(l: Line) -> Self {
        MemorySample { t: l.t, used_bytes: l.u, total_bytes: l.tot }
    }
}

/// Pourcentage d'utilisation `used / total * 100`. Garde-fou : `total == 0 => 0.0`.
///
/// Helper **pur et testable** cote Rust (garde-fou du ratio). Le `%` affiche est recalcule
/// cote webview (presentation), d'ou l'`allow(dead_code)` : la prod ne l'appelle pas directement.
#[allow(dead_code)]
pub fn used_pct(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64) * 100.0
    }
}

/// Epoch en secondes (horloge murale). `0` si l'horloge est anterieure a l'epoch (defensif).
pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Lit un echantillon instantane de la RAM via `sysinfo` (**memoire seule**, pas d'enumeration
/// des process). `used_memory()` = RAM reellement utilisee (hors cache/buffers).
pub fn read_sample() -> MemorySample {
    let mut sys = System::new_with_specifics(
        RefreshKind::nothing().with_memory(MemoryRefreshKind::everything()),
    );
    sys.refresh_memory();
    MemorySample {
        t: now_secs(),
        used_bytes: sys.used_memory(),
        total_bytes: sys.total_memory(),
    }
}

/// Append un echantillon en fin de fichier JSONL (une ligne compacte). Cree le dossier parent et
/// le fichier au besoin.
pub fn append_sample(path: &Path, sample: &MemorySample) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(&Line::from(sample)).map_err(std::io::Error::other)?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{line}")?;
    Ok(())
}

/// Lit l'historique persistant, **trie par `t` croissant**. Fichier absent -> `Vec` vide (defensif,
/// pas d'erreur). Les lignes illisibles (corrompues) sont **ignorees** sans planter.
pub fn read_history(path: &Path) -> Vec<MemorySample> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut out: Vec<MemorySample> = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str::<Line>(&l).ok())
        .map(MemorySample::from)
        .collect();
    out.sort_by_key(|s| s.t);
    out
}

/// Compaction : reecrit le fichier en ne gardant que les points de la fenetre de retention
/// (`t >= now - retention_secs`). Ecriture **atomique** (ecrit un `.tmp` filtre puis `rename`
/// par-dessus). Fichier absent -> no-op.
pub fn compact(path: &Path, retention_secs: i64, now: i64) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let cutoff = now - retention_secs;
    let kept: Vec<MemorySample> = read_history(path)
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

/// Etat du log memoire porte par `AppState` : chemin resolu du fichier JSONL persistant. Le `Mutex`
/// qui l'enveloppe **serialise** les acces entre le thread sampler (ecrit) et la commande (lit).
#[derive(Debug, Default)]
pub struct MemoryLog {
    pub path: PathBuf,
}

impl MemoryLog {
    /// Construit le log a partir du repertoire de donnees de l'app (joint le nom de fichier).
    pub fn in_dir(dir: &Path) -> Self {
        MemoryLog { path: dir.join(HISTORY_FILE) }
    }
}

/// Lance le sampler RAM dans un **thread detache** (patron `mqtt_sub::start`). Ne bloque pas ;
/// **ne panique jamais** (toute erreur I/O est loggee et la boucle continue). Detache => meurt avec
/// le process (aucun handling `Exit` requis).
pub fn start_sampler(app: AppHandle) {
    std::thread::spawn(move || run_sampler(app));
}

fn run_sampler(app: AppHandle) {
    let path = {
        let state = app.state::<AppState>();
        let log = state.memory.lock().unwrap();
        log.path.clone()
    };
    // Compaction au demarrage : borne le fichier des le lancement (execution continue sur des jours).
    {
        let state = app.state::<AppState>();
        let _guard = state.memory.lock().unwrap();
        if let Err(e) = compact(&path, RETENTION_SECS, now_secs()) {
            eprintln!("[iakatc-tray] compaction memoire initiale echouee: {e}");
        }
    }
    let mut writes: u64 = 0;
    loop {
        // Un premier echantillon est pris immediatement (point rapide apres lancement).
        let sample = read_sample();
        {
            let state = app.state::<AppState>();
            let _guard = state.memory.lock().unwrap();
            if let Err(e) = append_sample(&path, &sample) {
                eprintln!("[iakatc-tray] append echantillon memoire echoue: {e}");
            }
            writes = writes.wrapping_add(1);
            if writes.is_multiple_of(COMPACT_EVERY) {
                if let Err(e) = compact(&path, RETENTION_SECS, now_secs()) {
                    eprintln!("[iakatc-tray] compaction memoire echouee: {e}");
                }
            }
        }
        // Croissance live sans polling : la fenetre de details ecoute cet evenement.
        let _ = app.emit(MEMORY_EVENT, &sample);
        std::thread::sleep(Duration::from_secs(SAMPLE_INTERVAL_SECS));
    }
}

/// Commande : historique memoire persistant de la fenetre de retention, trie par `t` croissant.
/// Aucun parametre (la retention borne deja la taille). Fichier absent -> serie vide (defensif).
#[tauri::command]
pub fn get_memory_history(state: tauri::State<'_, AppState>) -> Vec<MemorySample> {
    let log = state.memory.lock().unwrap();
    read_history(&log.path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique pour un test, avec le fichier d'historique dedans.
    fn tmp_history(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("itc-mem-{name}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(HISTORY_FILE)
    }

    fn sample(t: i64, u: u64, tot: u64) -> MemorySample {
        MemorySample { t, used_bytes: u, total_bytes: tot }
    }

    #[test]
    fn used_pct_gardefou_et_ratio() {
        assert_eq!(used_pct(0, 0), 0.0);
        assert_eq!(used_pct(8, 16), 50.0);
        assert_eq!(used_pct(16, 16), 100.0);
    }

    #[test]
    fn round_trip_append_read_trie() {
        let path = tmp_history("rt");
        // Appends dans le desordre : read_history doit trier par t croissant.
        append_sample(&path, &sample(30, 2, 16)).unwrap();
        append_sample(&path, &sample(10, 1, 16)).unwrap();
        append_sample(&path, &sample(20, 3, 16)).unwrap();
        let h = read_history(&path);
        assert_eq!(h.iter().map(|s| s.t).collect::<Vec<_>>(), vec![10, 20, 30]);
        assert_eq!(h[0].used_bytes, 1);
        assert_eq!(h[2].used_bytes, 2);
    }

    #[test]
    fn read_history_fichier_absent_vide() {
        let path = std::env::temp_dir().join("itc-mem-absent-xyz/nope.jsonl");
        assert!(read_history(&path).is_empty());
    }

    #[test]
    fn ligne_corrompue_ignoree() {
        let path = tmp_history("corrupt");
        append_sample(&path, &sample(10, 1, 16)).unwrap();
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            writeln!(f, "pas du json valide").unwrap();
        }
        append_sample(&path, &sample(20, 2, 16)).unwrap();
        let h = read_history(&path);
        assert_eq!(h.iter().map(|s| s.t).collect::<Vec<_>>(), vec![10, 20]);
    }

    #[test]
    fn compact_retire_les_vieux_points_garde_les_recents() {
        let path = tmp_history("compact");
        let now = 1_000_000;
        append_sample(&path, &sample(now - 100_000, 1, 16)).unwrap(); // hors fenetre 24 h
        append_sample(&path, &sample(now - 100, 2, 16)).unwrap(); // dans la fenetre
        append_sample(&path, &sample(now, 3, 16)).unwrap(); // dans la fenetre
        compact(&path, RETENTION_SECS, now).unwrap();
        let h = read_history(&path);
        assert_eq!(h.len(), 2);
        assert!(h.iter().all(|s| s.t >= now - RETENTION_SECS));
    }

    #[test]
    fn compact_fichier_absent_noop() {
        let path = std::env::temp_dir().join("itc-mem-absent-compact-zzz/none.jsonl");
        compact(&path, RETENTION_SECS, 1_000_000).unwrap();
    }

    #[test]
    fn sample_serialise_en_camel_case() {
        let json = serde_json::to_string(&sample(5, 8, 16)).unwrap();
        assert!(json.contains("\"usedBytes\":8"), "{json}");
        assert!(json.contains("\"totalBytes\":16"), "{json}");
    }

    #[test]
    fn ligne_disque_utilise_des_cles_courtes() {
        // La persistance disque limite la taille via des cles courtes t/u/tot.
        let line = serde_json::to_string(&Line::from(&sample(5, 8, 16))).unwrap();
        assert!(line.contains("\"u\":8"), "{line}");
        assert!(line.contains("\"tot\":16"), "{line}");
    }
}
