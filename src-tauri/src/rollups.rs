//! rollups — rollups quotidiens `(jour, projet, provider, agent, modele)` : Travail + Volume total,
//! persistes SANS limite de retention (D2 de `specs/instructions/feature-memoire-historique.md`).
//!
//! Pourquoi (D1) : Claude Code purge les transcripts de plus de 30 jours (`cleanupPeriodDays`,
//! `subagents/` compris). Les deux grandeurs nommees de L0 (« Travail », « Volume total »,
//! `iakatc-core::measure::claude/codex::scan_*_daily`) ne survivraient pas a cette purge sans ce
//! cache. Le store **ne recalcule rien de nouveau** : il conserve un resultat que la source va
//! detruire (limite de D2 de `feature-app-analytics.md`, toujours vraie).
//!
//! **Idempotence (D4)** : un jour REVOLU (`< today`) est **fige une fois** — sa ligne n'est plus
//! jamais recalculee une fois ecrite. Le jour COURANT (`== today`) est **recalcule et ecrase** a
//! chaque generation. Deux generations consecutives sans que le temps avance produisent donc un
//! fichier BYTE-A-BYTE identique (les jours revolus sont intouches, le jour courant reproduit la
//! meme valeur car la source n'a pas change). Tant qu'un jour est encore present dans les
//! transcripts, la source fait foi (lecture directe) ; le rollup n'est qu'un filet pour les jours
//! disparus.
//!
//! Ecriture atomique (`.tmp` + `rename`), meme patron que `memory.rs::compact` / `quota_history.rs`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use iakatc_core::measure::claude::{claude_projects_dir, scan_claude_daily};
use iakatc_core::measure::codex::{codex_sessions_dir, scan_codex_daily};
use iakatc_core::measure::DailyMeasurement;

use crate::state::AppState;

/// Nom du fichier de rollups, joint au repertoire de donnees de l'app.
pub const ROLLUPS_FILE: &str = "daily-rollups.jsonl";

/// Une ligne de rollup quotidien : cle `(day, project, provider, agent, model)`, mesures
/// `work`/`volume` (miroir Rust des grandeurs nommees « Travail » / « Volume total »).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyRollup {
    pub day: String,
    pub project: String,
    pub provider: String,
    pub agent: String,
    /// Reserve pour L2 (ventilation par modele) : toujours `None` tant que L2 n'est pas livre.
    pub model: Option<String>,
    pub work: u64,
    pub volume: u64,
}

/// Representation disque compacte (cles courtes, patron `memory.rs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Line {
    d: String,
    p: String,
    pr: String,
    a: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    m: Option<String>,
    w: u64,
    v: u64,
}

impl From<&DailyRollup> for Line {
    fn from(r: &DailyRollup) -> Self {
        Line {
            d: r.day.clone(),
            p: r.project.clone(),
            pr: r.provider.clone(),
            a: r.agent.clone(),
            m: r.model.clone(),
            w: r.work,
            v: r.volume,
        }
    }
}

impl From<Line> for DailyRollup {
    fn from(l: Line) -> Self {
        DailyRollup {
            day: l.d,
            project: l.p,
            provider: l.pr,
            agent: l.a,
            model: l.m,
            work: l.w,
            volume: l.v,
        }
    }
}

/// Cle d'identite d'une ligne de rollup (sans les mesures) : sert au merge idempotent (D4).
type RollupKey = (String, String, String, String, Option<String>);

fn rollup_key(r: &DailyRollup) -> RollupKey {
    (
        r.day.clone(),
        r.project.clone(),
        r.provider.clone(),
        r.agent.clone(),
        r.model.clone(),
    )
}

fn to_rollup(m: DailyMeasurement) -> DailyRollup {
    DailyRollup {
        day: m.day,
        project: m.project,
        provider: m.provider.code().to_string(),
        agent: m.agent.code().to_string(),
        model: m.model,
        work: m.work,
        volume: m.volume,
    }
}

/// Genere les rollups FRAIS pour tous les jours actuellement visibles dans les transcripts (tous
/// providers). **Pur et testable** (dossiers en parametre, comme `history::build_history`).
/// Dossier absent (`None`) -> aucune contribution de ce provider (pas d'erreur).
pub fn generate_rollups(claude_dir: Option<&Path>, codex_dir: Option<&Path>) -> Vec<DailyRollup> {
    let mut out = Vec::new();
    if let Some(dir) = claude_dir {
        out.extend(scan_claude_daily(dir).into_iter().map(to_rollup));
    }
    if let Some(dir) = codex_dir {
        out.extend(scan_codex_daily(dir).into_iter().map(to_rollup));
    }
    out
}

/// Lit les rollups persistes, **tries par jour croissant** (puis projet/provider/agent pour un
/// ordre stable). Fichier absent -> `Vec` vide (defensif). Lignes corrompues **ignorees**.
pub fn read_rollups(path: &Path) -> Vec<DailyRollup> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut out: Vec<DailyRollup> = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str::<Line>(&l).ok())
        .map(DailyRollup::from)
        .collect();
    out.sort_by(|a, b| {
        a.day
            .cmp(&b.day)
            .then(a.project.cmp(&b.project))
            .then(a.provider.cmp(&b.provider))
            .then(a.agent.cmp(&b.agent))
    });
    out
}

/// Fusionne `fresh` (rollups fraichement calcules) dans le fichier persistant (D4, idempotent) :
/// - une ligne du jour `today` **remplace toujours** la ligne existante de meme cle ;
/// - une ligne d'un jour revolu (`day != today`) n'est ecrite **que si sa cle n'existe pas deja**
///   (fige une fois — les jours revolus ne sont jamais recalcules).
///
/// Ecriture atomique (`.tmp` + `rename`). Cree le dossier parent au besoin.
pub fn apply_rollups(path: &Path, today: &str, fresh: Vec<DailyRollup>) -> std::io::Result<()> {
    let mut by_key: HashMap<RollupKey, DailyRollup> = read_rollups(path)
        .into_iter()
        .map(|r| (rollup_key(&r), r))
        .collect();

    for row in fresh {
        let key = rollup_key(&row);
        if row.day == today {
            by_key.insert(key, row); // Jour courant : toujours recalcule/ecrase.
        } else {
            by_key.entry(key).or_insert(row); // Jour revolu : fige une fois.
        }
    }

    let mut all: Vec<DailyRollup> = by_key.into_values().collect();
    all.sort_by(|a, b| {
        a.day
            .cmp(&b.day)
            .then(a.project.cmp(&b.project))
            .then(a.provider.cmp(&b.provider))
            .then(a.agent.cmp(&b.agent))
    });

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        for r in &all {
            let line = serde_json::to_string(&Line::from(r)).map_err(std::io::Error::other)?;
            writeln!(f, "{line}")?;
        }
        f.flush()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Convertit un epoch (secondes UTC) en date civile `YYYY-MM-DD` (algorithme Howard Hinnant,
/// proleptique gregorien — pas de dependance a une crate date, coherent avec `day_of` d'iatc-core
/// qui decoupe deja des timestamps ISO sans lib externe). PUR/testable.
pub fn epoch_to_date(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Jours depuis l'epoch -> `(annee, mois, jour)`. Port direct de l'algorithme public domain de
/// Howard Hinnant (`civil_from_days`), proleptique gregorien, valide sur toute la plage `i64`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Etat du log de rollups porte par `AppState` : chemin resolu. Le `Mutex` **serialise** les trois
/// ecrivains potentiels (declenchement a l'ouverture de la vue analytics, thread quotidien de
/// fond) et le lecteur (commande) — meme patron que `memory::MemoryLog` / `quota_history::QuotaLog`.
#[derive(Debug, Default)]
pub struct RollupsLog {
    pub path: PathBuf,
}

impl RollupsLog {
    pub fn in_dir(dir: &Path) -> Self {
        RollupsLog { path: dir.join(ROLLUPS_FILE) }
    }
}

/// Genere les rollups frais depuis les transcripts et les fusionne dans le fichier (D4).
/// Best-effort : erreur d'I/O loggee, jamais de panique, jamais propagee au flux appelant (l'appel
/// se fait depuis un handler de commande ou un thread de fond, ni l'un ni l'autre ne doit planter
/// pour un souci de rollup).
pub fn generate_and_apply(path: &Path) {
    let claude_dir = claude_projects_dir();
    let codex_dir = codex_sessions_dir();
    let fresh = generate_rollups(claude_dir.as_deref(), codex_dir.as_deref());
    let today = epoch_to_date(crate::memory::now_secs());
    if let Err(e) = apply_rollups(path, &today, fresh) {
        eprintln!("[iakatc-tray] generation des rollups quotidiens echouee: {e}");
    }
}

/// Declenche la generation des rollups a l'ouverture de la vue analytics (D5, « point de
/// rafraichissement existant »), sous le verrou qui serialise avec le thread quotidien de fond.
pub fn refresh_on_view_open(app: &tauri::AppHandle) {
    use tauri::Manager;
    let path = {
        let state = app.state::<AppState>();
        let log = state.rollups.lock().unwrap();
        log.path.clone()
    };
    generate_and_apply(&path);
}

/// Lance le generateur quotidien de rollups dans un **thread detache** : une generation
/// immediate au demarrage (couvre le cas ou la vue analytics n'est jamais ouverte), puis une
/// generation toutes les 24 h. Ne panique jamais (`generate_and_apply` est deja defensif).
pub fn start_daily_scheduler(app: tauri::AppHandle) {
    use tauri::Manager;
    std::thread::spawn(move || loop {
        let path = {
            let state = app.state::<AppState>();
            let log = state.rollups.lock().unwrap();
            log.path.clone()
        };
        generate_and_apply(&path);
        std::thread::sleep(std::time::Duration::from_secs(86_400));
    });
}

/// Commande : rollups quotidiens persistes, tries. Fichier absent -> serie vide (defensif).
#[tauri::command]
pub fn get_daily_rollups(state: tauri::State<'_, AppState>) -> Vec<DailyRollup> {
    let log = state.rollups.lock().unwrap();
    read_rollups(&log.path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("itc-rollups-{name}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(ROLLUPS_FILE)
    }

    fn row(day: &str, project: &str, work: u64, volume: u64) -> DailyRollup {
        DailyRollup {
            day: day.to_string(),
            project: project.to_string(),
            provider: "claude".to_string(),
            agent: "coordinator".to_string(),
            model: None,
            work,
            volume,
        }
    }

    #[test]
    fn epoch_to_date_dates_connues() {
        assert_eq!(epoch_to_date(0), "1970-01-01");
        assert_eq!(epoch_to_date(86_400), "1970-01-02");
        // 2026-09-17T00:00:00Z (verifie via date -u -d @1789603200 sur un systeme de reference).
        assert_eq!(epoch_to_date(1_789_603_200), "2026-09-17");
        // Annee bissextile : 2024-02-29.
        assert_eq!(epoch_to_date(1_709_164_800), "2024-02-29");
    }

    #[test]
    fn read_rollups_fichier_absent_vide() {
        let path = std::env::temp_dir().join("itc-rollups-absent-xyz/nope.jsonl");
        assert!(read_rollups(&path).is_empty());
    }

    #[test]
    fn read_rollups_ligne_corrompue_ignoree() {
        let path = tmp_path("corrupt");
        apply_rollups(&path, "2026-01-02", vec![row("2026-01-01", "p", 1, 1)]).unwrap();
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            writeln!(f, "pas du json valide").unwrap();
        }
        let h = read_rollups(&path);
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn apply_rollups_ecrit_le_jour_courant_et_fige_les_jours_revolus() {
        let path = tmp_path("freeze");
        let today = "2026-01-03";
        let fresh = vec![
            row("2026-01-01", "p", 100, 150),
            row("2026-01-02", "p", 200, 250),
            row(today, "p", 5, 5),
        ];
        apply_rollups(&path, today, fresh).unwrap();
        let h = read_rollups(&path);
        assert_eq!(h.len(), 3);

        // Deuxieme generation : le jour courant AVANCE (les tokens du jour ont grossi), les jours
        // revolus restent identiques meme si (par hypothese d'erreur) le scan renvoyait une autre
        // valeur pour eux -> non repris, jour fige.
        let fresh2 = vec![
            row("2026-01-01", "p", 999, 999), // ne doit PAS ecraser (fige)
            row("2026-01-02", "p", 999, 999), // ne doit PAS ecraser (fige)
            row(today, "p", 9, 9),            // DOIT ecraser (jour courant)
        ];
        apply_rollups(&path, today, fresh2).unwrap();
        let h2 = read_rollups(&path);
        assert_eq!(h2.len(), 3);
        let d1 = h2.iter().find(|r| r.day == "2026-01-01").unwrap();
        let d2 = h2.iter().find(|r| r.day == "2026-01-02").unwrap();
        let d3 = h2.iter().find(|r| r.day == today).unwrap();
        assert_eq!(d1.work, 100, "jour revolu fige, pas ecrase");
        assert_eq!(d2.work, 200, "jour revolu fige, pas ecrase");
        assert_eq!(d3.work, 9, "jour courant recalcule");
    }

    #[test]
    fn apply_rollups_idempotent_deux_generations_identiques_produisent_le_meme_fichier() {
        let path = tmp_path("idempotent");
        let today = "2026-01-05";
        let fresh = vec![
            row("2026-01-04", "alpha", 10, 20),
            row(today, "alpha", 3, 4),
            row(today, "beta", 7, 9),
        ];
        apply_rollups(&path, today, fresh.clone()).unwrap();
        let bytes1 = std::fs::read(&path).unwrap();

        // Meme generation rejouee (source inchangee, temps immobile) : fichier IDENTIQUE.
        apply_rollups(&path, today, fresh).unwrap();
        let bytes2 = std::fs::read(&path).unwrap();
        assert_eq!(bytes1, bytes2, "un recalcul identique doit produire un fichier identique");
    }

    #[test]
    fn apply_rollups_jour_partiellement_ecrit_puis_complete_converge() {
        // Le jour courant grossit au fil des generations successives (nouveaux tours) : chaque
        // generation doit REMPLACER, jamais s'additionner a, la ligne precedente du jour courant.
        let path = tmp_path("converge");
        let today = "2026-02-01";
        apply_rollups(&path, today, vec![row(today, "p", 10, 15)]).unwrap();
        apply_rollups(&path, today, vec![row(today, "p", 10, 15)]).unwrap(); // pas de double-compte
        apply_rollups(&path, today, vec![row(today, "p", 40, 60)]).unwrap(); // grossit puis converge
        let h = read_rollups(&path);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].work, 40);
        assert_eq!(h[0].volume, 60);
    }

    #[test]
    fn apply_rollups_series_triees() {
        let path = tmp_path("sorted");
        apply_rollups(
            &path,
            "2026-01-10",
            vec![
                row("2026-01-10", "zed", 1, 1),
                row("2026-01-09", "alpha", 1, 1),
                row("2026-01-10", "alpha", 1, 1),
            ],
        )
        .unwrap();
        let h = read_rollups(&path);
        let days: Vec<&str> = h.iter().map(|r| r.day.as_str()).collect();
        assert_eq!(days, vec!["2026-01-09", "2026-01-10", "2026-01-10"]);
    }

    #[test]
    fn generate_rollups_dossiers_absents_liste_vide() {
        assert!(generate_rollups(None, None).is_empty());
    }

    #[test]
    fn generate_rollups_reconcilie_avec_le_scan_direct_sur_fixtures() {
        // Reconciliation (comportement attendu de l'instruction) : les totaux d'un jour dans le
        // rollup coincident avec le scan direct de ce jour, tant que la source existe.
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../specs/mock/claude_projects");
        let rows = generate_rollups(Some(&dir), None);
        assert!(!rows.is_empty(), "fixture attendue non vide");
        let direct = iakatc_core::measure::claude::scan_claude_daily(&dir);
        let mut direct_total: HashMap<(String, String), u64> = HashMap::new();
        for d in &direct {
            *direct_total.entry((d.day.clone(), d.project.clone())).or_insert(0) += d.work;
        }
        let mut rollup_total: HashMap<(String, String), u64> = HashMap::new();
        for r in &rows {
            *rollup_total.entry((r.day.clone(), r.project.clone())).or_insert(0) += r.work;
        }
        assert_eq!(rollup_total, direct_total);
    }

    #[test]
    fn line_utilise_des_cles_courtes_et_omet_model_absent() {
        let line = serde_json::to_string(&Line::from(&row("2026-01-01", "p", 10, 20))).unwrap();
        assert!(line.contains("\"d\":\"2026-01-01\""), "{line}");
        assert!(line.contains("\"w\":10"), "{line}");
        assert!(!line.contains("\"m\""), "model absent doit etre omis: {line}");
    }
}
