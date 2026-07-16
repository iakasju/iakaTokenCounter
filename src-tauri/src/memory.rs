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
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use sysinfo::{MemoryRefreshKind, RefreshKind, System};

/// Nom du fichier d'historique persistant, joint au repertoire de donnees de l'app.
pub const HISTORY_FILE: &str = "memory-history.jsonl";

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
