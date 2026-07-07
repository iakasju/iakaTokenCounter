//! measure::claude — conso Claude Code depuis les transcripts JSONL de session.
//!
//! ┌─────────────────────────────────────────────────────────────────────────────────────┐
//! │ PORTE de IakaCockpit/src-tauri/src/economy.rs @ 539ea7f (copie Rust->Rust, D2 de      │
//! │ specs/instructions/feature-collecteur-logs.md). Les wrappers `#[tauri::command]`      │
//! │ (`portfolio_economy`, `portfolio_activity`) ont ete retires ; les fonctions pures et  │
//! │ leurs tests sont conserves a l'identique (ils valident le portage). Ajout iakatc : le │
//! │ fold de mesure par (projet, agent) `scan_claude_measurements` (section « Mesure »).   │
//! └─────────────────────────────────────────────────────────────────────────────────────┘
//!
//! Lit les transcripts JSONL de session (`~/.claude/projects/<escaped>/<sid>.jsonl`) et
//! somme les tokens (`message.usage`) PAR PROJET (cle = dernier segment du `cwd` de chaque
//! record). Separe coordinateur (tours principaux) vs delegues (`isSidechain`). LECTURE
//! SEULE, defensif (une ligne invalide est ignoree, jamais de panique).

use super::{Agent, Measurement, Provider, Tokens};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// Cout agrege d'un projet (miroir TS `ProjectEconomy`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectEconomy {
    pub project: String,
    pub input: u64,
    pub output: u64,
    /// Tokens de sortie des tours du coordinateur (non-sidechain).
    pub coord: u64,
    /// Tokens de sortie des tours de sous-agents delegues (sidechain).
    pub sub: u64,
}

/// Accumulateur par projet : (input, output, coord, sub).
type Acc = HashMap<String, (u64, u64, u64, u64)>;

/// Dernier segment d'un cwd = nom de projet. Coupe sur `/` ET `\` pour gerer les vieux
/// transcripts Windows (`C:\iakaVODdash` -> `iakaVODdash`, `/a/b/iaka-demo` -> `iaka-demo`).
/// Defensif : chaine vide -> `None`, segments vides (separateurs de fin) ignores.
pub(crate) fn project_of(cwd: &str) -> Option<String> {
    cwd.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .map(str::to_string)
}

/// Integre UNE ligne JSONL (record assistant avec `message.usage`) dans l'accumulateur.
/// PUR/testable. Ignore proprement tout record non pertinent.
pub fn fold_line(acc: &mut Acc, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return,
    };
    if v.get("type").and_then(Value::as_str) != Some("assistant") {
        return;
    }
    let usage = match v.get("message").and_then(|m| m.get("usage")) {
        Some(u) => u,
        None => return,
    };
    let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    let input = n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens");
    let output = n("output_tokens");
    if input == 0 && output == 0 {
        return;
    }
    let project = match v.get("cwd").and_then(Value::as_str).and_then(project_of) {
        Some(p) => p,
        None => return,
    };
    let sidechain = v
        .get("isSidechain")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let e = acc.entry(project).or_default();
    e.0 += input;
    e.1 += output;
    if sidechain {
        e.3 += output;
    } else {
        e.2 += output;
    }
}

/// Convertit l'accumulateur en liste triee (cout total desc), bornee a `top`.
pub fn finalize(acc: Acc, top: usize) -> Vec<ProjectEconomy> {
    let mut out: Vec<ProjectEconomy> = acc
        .into_iter()
        .map(|(project, (input, output, coord, sub))| ProjectEconomy {
            project,
            input,
            output,
            coord,
            sub,
        })
        .collect();
    out.sort_by_key(|p| std::cmp::Reverse(p.input + p.output));
    out.truncate(top);
    out
}

/// Scanne un dossier `projects/` (chaque sous-dossier = un cwd escape, chaque `.jsonl` =
/// une session) et agrege. Defensif : un fichier/dir illisible est ignore.
pub fn scan_projects_dir(projects_dir: &Path, top: usize) -> Vec<ProjectEconomy> {
    let mut acc: Acc = HashMap::new();
    let dirs = match std::fs::read_dir(projects_dir) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    for sess_dir in dirs.flatten() {
        let files = match std::fs::read_dir(sess_dir.path()) {
            Ok(f) => f,
            Err(_) => continue,
        };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&p) {
                for line in content.lines() {
                    fold_line(&mut acc, line);
                }
            }
        }
    }
    finalize(acc, top)
}

// ============================ Ventilation tokens/jour/projet (L21 D) ============================
//
// Pour la visu « travail passe » (scatter-timeline), on a besoin d'une serie tokens PAR JOUR et
// PAR PROJET — pas seulement des totaux. Algo calque sur `naonedge-dashboard/scan.js getTokenStats`
// (byDay) : par ligne, somme `input + output + cache_creation` **HORS `cache_read`** (ecart ASSUME
// vs `fold_line` ci-dessus qui inclut `cache_read` pour les TOTAUX — ici on applique la regle
// dashboard), cle jour = prefixe `YYYY-MM-DD` du `timestamp`. LECTURE SEULE, defensif.

/// Tokens d'UN jour pour un projet (miroir TS `DayTokens`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DayTokens {
    pub date: String,
    pub tokens: u64,
}

/// Serie d'activite d'un projet (jours tries croissants) (miroir TS `ProjectActivity`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectActivity {
    pub project: String,
    pub days: Vec<DayTokens>,
}

/// Accumulateur d'activite : projet -> (jour -> tokens).
type ActAcc = HashMap<String, HashMap<String, u64>>;

/// Extrait le prefixe `YYYY-MM-DD` d'un timestamp ISO (`2026-06-30T12:00:00Z` -> `2026-06-30`).
/// Renvoie `None` si la forme n'est pas une date (defensif — pas de bulle non datable).
fn day_of(ts: &str) -> Option<String> {
    if ts.len() < 10 {
        return None;
    }
    let head = &ts[..10];
    let bytes = head.as_bytes();
    let ok = bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit);
    ok.then(|| head.to_string())
}

/// Integre UNE ligne JSONL dans l'accumulateur d'activite (byDay, HORS `cache_read`).
/// PUR/testable. Ignore proprement tout record non pertinent ou non date.
pub fn fold_activity_line(acc: &mut ActAcc, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return,
    };
    if v.get("type").and_then(Value::as_str) != Some("assistant") {
        return;
    }
    let usage = match v.get("message").and_then(|m| m.get("usage")) {
        Some(u) => u,
        None => return,
    };
    let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    // Regle dashboard : input + output + cache_creation, **SANS cache_read**.
    let sum = n("input_tokens") + n("output_tokens") + n("cache_creation_input_tokens");
    if sum == 0 {
        return;
    }
    let project = match v.get("cwd").and_then(Value::as_str).and_then(project_of) {
        Some(p) => p,
        None => return,
    };
    let day = match v.get("timestamp").and_then(Value::as_str).and_then(day_of) {
        Some(d) => d,
        None => return,
    };
    *acc.entry(project).or_default().entry(day).or_insert(0) += sum;
}

/// Convertit l'accumulateur d'activite en liste : jours tries croissants, projets tries par
/// total tokens decroissant, borne a `top`.
pub fn finalize_activity(acc: ActAcc, top: usize) -> Vec<ProjectActivity> {
    let mut out: Vec<ProjectActivity> = acc
        .into_iter()
        .map(|(project, by_day)| {
            let mut days: Vec<DayTokens> = by_day
                .into_iter()
                .map(|(date, tokens)| DayTokens { date, tokens })
                .collect();
            days.sort_by(|a, b| a.date.cmp(&b.date));
            ProjectActivity { project, days }
        })
        .collect();
    out.sort_by_key(|p| std::cmp::Reverse(p.days.iter().map(|d| d.tokens).sum::<u64>()));
    out.truncate(top);
    out
}

/// Scanne un dossier `projects/` et agrege l'activite byDay/projet. Defensif.
pub fn scan_projects_activity(projects_dir: &Path, top: usize) -> Vec<ProjectActivity> {
    let mut acc: ActAcc = HashMap::new();
    let dirs = match std::fs::read_dir(projects_dir) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    for sess_dir in dirs.flatten() {
        let files = match std::fs::read_dir(sess_dir.path()) {
            Ok(f) => f,
            Err(_) => continue,
        };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&p) {
                for line in content.lines() {
                    fold_activity_line(&mut acc, line);
                }
            }
        }
    }
    finalize_activity(acc, top)
}

/// Repertoire des transcripts Claude Code : `<home>/.claude/projects`.
pub fn claude_projects_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(Path::new(&home).join(".claude").join("projects"))
}

// ================================ Mesure par (projet, agent) ================================
//
// Ajout iakatc (hors economy.rs) : le contrat MQTT (§ 3.1) demande, PAR agent, les 4 codes
// `input_tokens` / `output_tokens` / `cache_tokens` / `used_tokens`. economy.rs n'attribue pas
// l'input par agent (il ne separe que l'output coord/sub). On refait donc un fold dedie qui
// ventile TOUT (`input`, `output`, `cache`) par `(project, agent)`, en reutilisant `project_of`
// et la meme regle de tokens que `fold_line` (input inclut les caches). LECTURE SEULE, defensif.

/// Cle d'accumulation de mesure Claude : `(project, agent)`.
type MeasAcc = HashMap<(String, Agent), Tokens>;

/// Integre UNE ligne JSONL dans l'accumulateur de mesure par `(project, agent)`. PUR/testable.
pub fn fold_measure_line(acc: &mut MeasAcc, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return,
    };
    if v.get("type").and_then(Value::as_str) != Some("assistant") {
        return;
    }
    let usage = match v.get("message").and_then(|m| m.get("usage")) {
        Some(u) => u,
        None => return,
    };
    let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    let cache = n("cache_creation_input_tokens") + n("cache_read_input_tokens");
    let input = n("input_tokens") + cache;
    let output = n("output_tokens");
    if input == 0 && output == 0 {
        return;
    }
    let project = match v.get("cwd").and_then(Value::as_str).and_then(project_of) {
        Some(p) => p,
        None => return,
    };
    let agent = if v
        .get("isSidechain")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        Agent::Subagent
    } else {
        Agent::Coordinator
    };
    acc.entry((project, agent)).or_default().add(&Tokens {
        input,
        output,
        cache,
    });
}

/// Convertit l'accumulateur de mesure en `Vec<Measurement>` (provider = Claude).
fn finalize_measurements(acc: MeasAcc) -> Vec<Measurement> {
    let mut out: Vec<Measurement> = acc
        .into_iter()
        .map(|((project, agent), tokens)| Measurement {
            project,
            provider: Provider::Claude,
            agent,
            tokens,
        })
        .collect();
    // Tri deterministe (facilite les tests et la lisibilite des publications).
    out.sort_by(|a, b| {
        a.project
            .cmp(&b.project)
            .then(a.agent.code().cmp(b.agent.code()))
    });
    out
}

/// Scanne le dossier `projects/` de Claude Code et produit les mesures par `(project, agent)`.
/// Defensif : dossier/fichier illisible ignore, jamais de panique.
pub fn scan_claude_measurements(projects_dir: &Path) -> Vec<Measurement> {
    let mut acc: MeasAcc = HashMap::new();
    let dirs = match std::fs::read_dir(projects_dir) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    for sess_dir in dirs.flatten() {
        let files = match std::fs::read_dir(sess_dir.path()) {
            Ok(f) => f,
            Err(_) => continue,
        };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&p) {
                for line in content.lines() {
                    fold_measure_line(&mut acc, line);
                }
            }
        }
    }
    finalize_measurements(acc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_of_prend_le_dernier_segment() {
        assert_eq!(
            project_of("/Users/x/work/iaka-demo"),
            Some("iaka-demo".into())
        );
        assert_eq!(project_of("/a/b/"), Some("b".into()));
        assert_eq!(project_of(""), None);
    }

    #[test]
    fn project_of_gere_les_cwd_windows() {
        // Vieux transcripts Windows : separateur antislash.
        assert_eq!(project_of(r"C:\iakaVODdash"), Some("iakaVODdash".into()));
        assert_eq!(
            project_of(r"C:\Users\x\work\iaka-demo"),
            Some("iaka-demo".into())
        );
        // Chemin mixte (slash + antislash).
        assert_eq!(project_of(r"/c/work\iaka-demo"), Some("iaka-demo".into()));
        // Separateur de fin (trailing) ignore.
        assert_eq!(project_of(r"C:\iakaVODdash\"), Some("iakaVODdash".into()));
        assert_eq!(project_of(r"\a\b\"), Some("b".into()));
    }

    #[test]
    fn fold_line_somme_input_caches_et_output() {
        let mut acc = Acc::new();
        fold_line(
            &mut acc,
            r#"{"type":"assistant","cwd":"/w/iaka-demo","message":{"usage":{"input_tokens":100,"cache_read_input_tokens":50,"output_tokens":20}}}"#,
        );
        let e = &acc["iaka-demo"];
        assert_eq!(e.0, 150); // input + caches
        assert_eq!(e.1, 20); // output
        assert_eq!(e.2, 20); // coord (non-sidechain)
        assert_eq!(e.3, 0);
    }

    #[test]
    fn fold_line_separe_coordinateur_et_delegues() {
        let mut acc = Acc::new();
        fold_line(
            &mut acc,
            r#"{"type":"assistant","cwd":"/w/p","message":{"usage":{"input_tokens":10,"output_tokens":5}}}"#,
        );
        fold_line(
            &mut acc,
            r#"{"type":"assistant","isSidechain":true,"cwd":"/w/p","message":{"usage":{"input_tokens":8,"output_tokens":3}}}"#,
        );
        let e = &acc["p"];
        assert_eq!(e.2, 5); // coord
        assert_eq!(e.3, 3); // sub
    }

    #[test]
    fn fold_line_ignore_non_assistant_et_sans_usage() {
        let mut acc = Acc::new();
        fold_line(
            &mut acc,
            r#"{"type":"user","cwd":"/w/p","message":{"content":"x"}}"#,
        );
        fold_line(
            &mut acc,
            r#"{"type":"assistant","cwd":"/w/p","message":{"content":[]}}"#,
        );
        fold_line(&mut acc, "pas du json");
        assert!(acc.is_empty());
    }

    #[test]
    fn finalize_trie_par_cout_total_et_borne() {
        let mut acc = Acc::new();
        acc.insert("a".into(), (10, 5, 5, 0));
        acc.insert("b".into(), (100, 50, 50, 0));
        acc.insert("c".into(), (1, 1, 1, 0));
        let v = finalize(acc, 2);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].project, "b");
        assert_eq!(v[1].project, "a");
    }

    // ---------------- Ventilation tokens/jour/projet (L21 D) ----------------

    #[test]
    fn day_of_extrait_le_prefixe_date() {
        assert_eq!(day_of("2026-06-30T12:00:00Z"), Some("2026-06-30".into()));
        assert_eq!(day_of("2026-06-30"), Some("2026-06-30".into()));
        assert_eq!(day_of("pas-une-date"), None);
        assert_eq!(day_of("2026/06/30T.."), None);
        assert_eq!(day_of(""), None);
    }

    #[test]
    fn fold_activity_somme_input_output_cache_creation_hors_cache_read() {
        let mut acc = ActAcc::new();
        fold_activity_line(
            &mut acc,
            r#"{"type":"assistant","timestamp":"2026-06-30T10:00:00Z","cwd":"/w/iaka-demo","message":{"usage":{"input_tokens":100,"output_tokens":20,"cache_creation_input_tokens":30,"cache_read_input_tokens":9999}}}"#,
        );
        // 100 + 20 + 30 = 150 ; cache_read (9999) EXCLU.
        assert_eq!(acc["iaka-demo"]["2026-06-30"], 150);
    }

    #[test]
    fn fold_activity_bucket_par_jour() {
        let mut acc = ActAcc::new();
        fold_activity_line(
            &mut acc,
            r#"{"type":"assistant","timestamp":"2026-06-29T23:00:00Z","cwd":"/w/p","message":{"usage":{"input_tokens":10,"output_tokens":0}}}"#,
        );
        fold_activity_line(
            &mut acc,
            r#"{"type":"assistant","timestamp":"2026-06-30T01:00:00Z","cwd":"/w/p","message":{"usage":{"input_tokens":5,"output_tokens":0}}}"#,
        );
        fold_activity_line(
            &mut acc,
            r#"{"type":"assistant","timestamp":"2026-06-30T02:00:00Z","cwd":"/w/p","message":{"usage":{"input_tokens":7,"output_tokens":0}}}"#,
        );
        assert_eq!(acc["p"]["2026-06-29"], 10);
        assert_eq!(acc["p"]["2026-06-30"], 12); // 5 + 7 cumules sur le jour
    }

    #[test]
    fn fold_activity_ignore_non_assistant_sans_usage_et_non_date() {
        let mut acc = ActAcc::new();
        fold_activity_line(
            &mut acc,
            r#"{"type":"user","timestamp":"2026-06-30T10:00:00Z","cwd":"/w/p","message":{"content":"x"}}"#,
        );
        fold_activity_line(
            &mut acc,
            r#"{"type":"assistant","cwd":"/w/p","message":{"usage":{"input_tokens":10}}}"#,
        ); // pas de timestamp
        fold_activity_line(
            &mut acc,
            r#"{"type":"assistant","timestamp":"2026-06-30T10:00:00Z","cwd":"/w/p","message":{"usage":{"cache_read_input_tokens":500}}}"#,
        ); // que du cache_read -> sum 0
        fold_activity_line(&mut acc, "pas du json");
        assert!(acc.is_empty());
    }

    #[test]
    fn finalize_activity_jours_tries_projets_par_total_et_borne() {
        let mut acc = ActAcc::new();
        acc.entry("a".into())
            .or_default()
            .insert("2026-06-30".into(), 5);
        acc.entry("a".into())
            .or_default()
            .insert("2026-06-28".into(), 3);
        acc.entry("b".into())
            .or_default()
            .insert("2026-06-30".into(), 100);
        acc.entry("c".into())
            .or_default()
            .insert("2026-06-30".into(), 1);
        let v = finalize_activity(acc, 2);
        assert_eq!(v.len(), 2); // borne top 2
        assert_eq!(v[0].project, "b"); // 100 > total a (8) > c (1)
        assert_eq!(v[1].project, "a");
        // Jours tries croissants pour a.
        assert_eq!(
            v[1].days
                .iter()
                .map(|d| d.date.as_str())
                .collect::<Vec<_>>(),
            vec!["2026-06-28", "2026-06-30"]
        );
    }

    // ---------------- Mesure par (projet, agent) (ajout iakatc) ----------------

    #[test]
    fn fold_measure_ventile_input_output_cache_par_agent() {
        let mut acc = MeasAcc::new();
        fold_measure_line(
            &mut acc,
            r#"{"type":"assistant","cwd":"/w/P","message":{"usage":{"input_tokens":100,"cache_creation_input_tokens":30,"cache_read_input_tokens":20,"output_tokens":40}}}"#,
        );
        let t = &acc[&("P".to_string(), Agent::Coordinator)];
        assert_eq!(t.input, 150); // 100 + 30 + 20
        assert_eq!(t.output, 40);
        assert_eq!(t.cache, 50); // 30 + 20
        assert_eq!(t.used(), 190); // input + output
    }

    #[test]
    fn fold_measure_separe_coordinateur_et_sous_agent() {
        let mut acc = MeasAcc::new();
        fold_measure_line(
            &mut acc,
            r#"{"type":"assistant","cwd":"/w/P","message":{"usage":{"input_tokens":10,"output_tokens":5}}}"#,
        );
        fold_measure_line(
            &mut acc,
            r#"{"type":"assistant","isSidechain":true,"cwd":"/w/P","message":{"usage":{"input_tokens":8,"output_tokens":3}}}"#,
        );
        assert_eq!(
            acc[&("P".to_string(), Agent::Coordinator)].used(),
            15 // 10 + 5
        );
        assert_eq!(
            acc[&("P".to_string(), Agent::Subagent)].used(),
            11 // 8 + 3
        );
    }

    #[test]
    fn finalize_measurements_provider_claude_et_tri_deterministe() {
        let mut acc = MeasAcc::new();
        acc.insert(
            ("Z".into(), Agent::Coordinator),
            Tokens {
                input: 1,
                output: 1,
                cache: 0,
            },
        );
        acc.insert(
            ("A".into(), Agent::Subagent),
            Tokens {
                input: 2,
                output: 2,
                cache: 0,
            },
        );
        let m = finalize_measurements(acc);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].project, "A"); // tri projet croissant
        assert!(m.iter().all(|x| x.provider == Provider::Claude));
    }
}
