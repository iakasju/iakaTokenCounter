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
//! Lit les transcripts JSONL de session (`~/.claude/projects/<escaped>/<sid>.jsonl` ET
//! `~/.claude/projects/<escaped>/<sid>/subagents/agent-<id>.jsonl`, cf. [`claude_transcript_files`])
//! et somme les tokens (`message.usage`) PAR PROJET. Separe coordinateur (tours principaux) vs
//! delegues (`isSidechain`). LECTURE SEULE, defensif (une ligne invalide est ignoree, jamais de
//! panique).
//!
//! ## Regle du « projet » (D4, `specs/instructions/feature-verite-des-chiffres.md`)
//!
//! Le nom de projet n'est PAS le nom du repertoire sous `~/.claude/projects` : il est extrait du
//! **contenu**, ligne par ligne, par [`project_of`] — **dernier segment du `cwd`** de chaque
//! enregistrement (pas le nom du dossier escape). Deux consequences assumees :
//!
//! - **Seau « hors projet »** : quand Claude Code tourne directement a la racine d'un dossier de
//!   portefeuille (`~/work`, `~/Desktop`...) plutot que dans un sous-projet, `project_of` renvoie
//!   le nom de cette racine (`work`, `Desktop`) — ce n'est pas un projet. Ces racines, listees
//!   explicitement dans [`PORTFOLIO_ROOTS`] (une constante nommee et isolee, pas une liste
//!   eparpillee ni une detection automatique par inclusion de chemins), sont regroupees sous le
//!   seau unique [`OUT_OF_PROJECT_BUCKET`].
//! - **Collision de feuilles assumee** : `/a/web` et `/b/web` fusionnent tous les deux sous
//!   `web` — non resolu ici (il faudrait changer la cle). L'infobulle d'un projet (cote webview)
//!   affiche le `cwd` complet (`ProjectEconomy::example_cwd`) pour lever l'ambiguite au cas par
//!   cas.

use super::{Agent, Measurement, Provider, Tokens};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
    /// Un `cwd` complet ayant contribue a ce projet (D4, infobulle) : leve l'ambiguite d'une
    /// collision de feuille (`/a/web` et `/b/web` -> `web`) et, pour le seau
    /// [`OUT_OF_PROJECT_BUCKET`], montre quelle racine de portefeuille est en cause. Chaine vide
    /// si aucun `cwd` exploitable n'a ete rencontre (defensif).
    #[serde(rename = "exampleCwd")]
    pub example_cwd: String,
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

/// Racines de portefeuille connues (D4) : quand Claude Code tourne a la racine d'un de ces
/// dossiers plutot que dans un sous-projet, `project_of` renvoie ce nom de racine — pas un vrai
/// projet. Liste EXPLICITE et ISOLEE : pas de detection automatique par inclusion de chemins
/// (ecarte, complexite sans rapport avec l'objet du lot), pas de table d'alias configurable
/// (ecarte, sur-ingenierie).
pub(crate) const PORTFOLIO_ROOTS: &[&str] = &["work", "Desktop"];

/// Seau ou tombent les projets issus d'une racine de portefeuille (D4).
pub(crate) const OUT_OF_PROJECT_BUCKET: &str = "hors projet";

/// Reduit un nom de projet BRUT (sortie de [`project_of`]) a sa forme affichee : identique, sauf
/// s'il s'agit d'une racine de portefeuille connue, auquel cas il tombe dans
/// [`OUT_OF_PROJECT_BUCKET`] (D4).
fn bucket_project(raw: String) -> String {
    if PORTFOLIO_ROOTS.contains(&raw.as_str()) {
        OUT_OF_PROJECT_BUCKET.to_string()
    } else {
        raw
    }
}

/// Liste RECURSIVEMENT tous les `*.jsonl` sous `projects_dir` (D1) : transcripts de session
/// principale (`<sid>.jsonl`) ET transcripts de sous-agents delegues
/// (`<sid>/subagents/agent-<id>.jsonl`), sans hypothese sur la profondeur — si l'editeur deplace
/// ou renomme `subagents/`, la marche continue de trouver les fichiers. Calque le patron deja
/// ecrit et eprouve de [`super::codex::walk_jsonl`] : pile de repertoires, `flatten()` sur les
/// entrees, repertoire illisible saute sans erreur. Ordre de retour NON garanti (les folds qui
/// consomment cette liste sont purement additifs, l'ordre n'a pas d'incidence).
pub fn claude_transcript_files(projects_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![projects_dir.to_path_buf()];
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

/// Extrait `message.id` d'une ligne JSONL, si present et de type chaine. PUR/testable, defensif :
/// JSON invalide, champ absent ou de mauvais type -> `None` (la ligne appelante restera alors
/// conservee sans dedup, cf. [`dedup_lines_by_message_id`] — on ne perd jamais de donnee par exces
/// de zele).
fn message_id_of(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    v.get("message")?
        .get("id")?
        .as_str()
        .map(str::to_string)
}

/// Deduplique les lignes d'UN fichier par `message.id` (D2) : un meme message d'assistant est
/// parfois reecrit sur plusieurs lignes (une par bloc de contenu), chacune reportant le meme
/// `usage` — on ne garde que la DERNIERE occurrence de chaque identifiant (strictement plus sur
/// que la premiere : certains cas rapportes dans l'ecosysteme montrent une premiere occurrence
/// partielle et une derniere complete ; ici les occurrences sont identiques, donc premiere ou
/// derniere revient au meme, a cout nul). Une ligne SANS `message.id` exploitable est conservee
/// TELLE QUELLE. Retourne les lignes retenues (ordre d'origine ; sans incidence, les folds qui les
/// consomment sont purement additifs).
fn dedup_lines_by_message_id(content: &str) -> Vec<&str> {
    let lines: Vec<&str> = content.lines().collect();
    let ids: Vec<Option<String>> = lines.iter().map(|l| message_id_of(l)).collect();

    let mut last_idx: HashMap<&str, usize> = HashMap::new();
    for (i, id) in ids.iter().enumerate() {
        if let Some(id) = id {
            last_idx.insert(id.as_str(), i);
        }
    }
    let keep: HashSet<usize> = last_idx.into_values().collect();

    lines
        .into_iter()
        .zip(ids)
        .enumerate()
        .filter(|(i, (_, id))| id.is_none() || keep.contains(i))
        .map(|(_, (line, _))| line)
        .collect()
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

/// Integre UN FICHIER complet dans l'accumulateur economie (D2) : deduplique ses lignes par
/// `message.id` puis delegue chaque ligne retenue a [`fold_line`], INCHANGE. `fold_line` reste
/// pur, public et couvert par sa propre batterie de tests ; cette fonction n'ajoute que la
/// dedup au niveau fichier au-dessus.
pub fn fold_file_economy(acc: &mut Acc, content: &str) {
    for line in dedup_lines_by_message_id(content) {
        fold_line(acc, line);
    }
}

/// Accumulateur d'exemples de `cwd` PAR PROJET BRUT (avant bucket D4) : plus petit `cwd`
/// rencontre (comparaison lexicographique -> deterministe quel que soit l'ordre de scan).
type CwdAcc = HashMap<String, String>;

/// Extrait le `cwd` d'UNE ligne JSONL, quel que soit son type. PUR/testable, defensif.
fn cwd_of_line(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    v.get("cwd").and_then(Value::as_str).map(str::to_string)
}

/// Integre UNE ligne dans l'accumulateur d'exemples de `cwd` (D4, infobulle) : cle = projet BRUT
/// (`project_of(cwd)`, avant bucket portefeuille), valeur = le plus petit `cwd` rencontre pour ce
/// projet (deterministe). PUR/testable, defensif : cwd absent/non exploitable -> ignore.
fn fold_cwd_line(acc: &mut CwdAcc, line: &str) {
    let cwd = match cwd_of_line(line) {
        Some(c) => c,
        None => return,
    };
    let raw_project = match project_of(&cwd) {
        Some(p) => p,
        None => return,
    };
    acc.entry(raw_project)
        .and_modify(|e| {
            if cwd < *e {
                *e = cwd.clone();
            }
        })
        .or_insert(cwd);
}

/// Le `cwd` complet a afficher en infobulle pour un projet BUCKETE (D4) : pour le seau
/// [`OUT_OF_PROJECT_BUCKET`], le plus petit `cwd` parmi toutes les racines de portefeuille
/// rencontrees (deterministe) ; sinon, l'exemple du projet lui-meme. Chaine vide si aucun
/// exemple connu (defensif).
fn example_cwd_for(bucketed_name: &str, cwd_examples: &CwdAcc) -> String {
    if bucketed_name == OUT_OF_PROJECT_BUCKET {
        PORTFOLIO_ROOTS
            .iter()
            .filter_map(|root| cwd_examples.get(*root))
            .min()
            .cloned()
            .unwrap_or_default()
    } else {
        cwd_examples.get(bucketed_name).cloned().unwrap_or_default()
    }
}

/// Applique le bucket portefeuille (D4) aux cles de l'accumulateur economie, en sommant les
/// entrees qui fusionnent (ex. `work` + `Desktop` -> `hors projet`).
fn bucketed(acc: Acc) -> Acc {
    let mut out: Acc = HashMap::new();
    for (project, (input, output, coord, sub)) in acc {
        let e = out.entry(bucket_project(project)).or_insert((0, 0, 0, 0));
        e.0 += input;
        e.1 += output;
        e.2 += coord;
        e.3 += sub;
    }
    out
}

/// Convertit l'accumulateur en liste triee (cout total desc), bornee a `top`. Applique le bucket
/// portefeuille (D4) et joint un `cwd` d'exemple par projet (infobulle).
pub fn finalize(acc: Acc, cwd_examples: &CwdAcc, top: usize) -> Vec<ProjectEconomy> {
    let acc = bucketed(acc);
    let mut out: Vec<ProjectEconomy> = acc
        .into_iter()
        .map(|(project, (input, output, coord, sub))| {
            let example_cwd = example_cwd_for(&project, cwd_examples);
            ProjectEconomy {
                project,
                input,
                output,
                coord,
                sub,
                example_cwd,
            }
        })
        .collect();
    out.sort_by_key(|p| std::cmp::Reverse(p.input + p.output));
    out.truncate(top);
    out
}

/// Scanne RECURSIVEMENT un dossier `projects/` (D1 : sessions principales ET sous-agents
/// `subagents/`) et agrege, dedupliquant chaque fichier par `message.id` (D2). Defensif : un
/// fichier/dir illisible est ignore.
pub fn scan_projects_dir(projects_dir: &Path, top: usize) -> Vec<ProjectEconomy> {
    let mut acc: Acc = HashMap::new();
    let mut cwd_examples: CwdAcc = HashMap::new();
    for p in claude_transcript_files(projects_dir) {
        if let Ok(content) = std::fs::read_to_string(&p) {
            fold_file_economy(&mut acc, &content);
            for line in content.lines() {
                fold_cwd_line(&mut cwd_examples, line);
            }
        }
    }
    finalize(acc, &cwd_examples, top)
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
/// `pub(crate)` : reutilise par `measure::codex` pour bucketer l'activite Codex par jour.
pub(crate) fn day_of(ts: &str) -> Option<String> {
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

/// Integre UN FICHIER complet dans l'accumulateur d'activite (D2) : deduplique ses lignes par
/// `message.id` puis delegue chaque ligne retenue a [`fold_activity_line`], INCHANGE.
pub fn fold_file_activity(acc: &mut ActAcc, content: &str) {
    for line in dedup_lines_by_message_id(content) {
        fold_activity_line(acc, line);
    }
}

/// Applique le bucket portefeuille (D4) aux cles de l'accumulateur d'activite, en sommant
/// jour par jour les entrees qui fusionnent.
fn bucketed_activity(acc: ActAcc) -> ActAcc {
    let mut out: ActAcc = HashMap::new();
    for (project, by_day) in acc {
        let entry = out.entry(bucket_project(project)).or_default();
        for (day, tokens) in by_day {
            *entry.entry(day).or_insert(0) += tokens;
        }
    }
    out
}

/// Convertit l'accumulateur d'activite en liste : jours tries croissants, projets tries par
/// total tokens decroissant, borne a `top`. Applique le bucket portefeuille (D4).
pub fn finalize_activity(acc: ActAcc, top: usize) -> Vec<ProjectActivity> {
    let acc = bucketed_activity(acc);
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

/// Scanne RECURSIVEMENT un dossier `projects/` (D1) et agrege l'activite byDay/projet,
/// dedupliquant chaque fichier par `message.id` (D2). Defensif.
pub fn scan_projects_activity(projects_dir: &Path, top: usize) -> Vec<ProjectActivity> {
    let mut acc: ActAcc = HashMap::new();
    for p in claude_transcript_files(projects_dir) {
        if let Ok(content) = std::fs::read_to_string(&p) {
            fold_file_activity(&mut acc, &content);
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

/// Cle d'accumulation de mesure Claude : `(project, agent)`. `pub(crate)` : reutilise par
/// `measure::cache` (D5, memo par fichier du scan mesure).
pub(crate) type MeasAcc = HashMap<(String, Agent), Tokens>;

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

/// Integre UN FICHIER complet dans l'accumulateur de mesure (D2) : deduplique ses lignes par
/// `message.id` puis delegue chaque ligne retenue a [`fold_measure_line`], INCHANGE. `pub(crate)` :
/// reutilise par `measure::cache` pour le resultat partiel memorise d'un fichier (D5).
pub(crate) fn fold_file_measure(acc: &mut MeasAcc, content: &str) {
    for line in dedup_lines_by_message_id(content) {
        fold_measure_line(acc, line);
    }
}

/// Applique le bucket portefeuille (D4) aux cles de l'accumulateur de mesure, en sommant les
/// `Tokens` des entrees qui fusionnent.
fn bucketed_measure(acc: MeasAcc) -> MeasAcc {
    let mut out: MeasAcc = HashMap::new();
    for ((project, agent), tokens) in acc {
        out.entry((bucket_project(project), agent))
            .or_default()
            .add(&tokens);
    }
    out
}

/// Convertit l'accumulateur de mesure en `Vec<Measurement>` (provider = Claude). Applique le
/// bucket portefeuille (D4). `pub(crate)` : reutilise par `measure::cache` (D5).
pub(crate) fn finalize_measurements(acc: MeasAcc) -> Vec<Measurement> {
    let acc = bucketed_measure(acc);
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

/// Scanne RECURSIVEMENT le dossier `projects/` de Claude Code (D1) et produit les mesures par
/// `(project, agent)`, dedupliquant chaque fichier par `message.id` (D2). Defensif : dossier/
/// fichier illisible ignore, jamais de panique. Variante NON memoisee — reste disponible pour la
/// GUI et les tests ; la variante memoisee (`measure::cache::scan_claude_measurements_cached`,
/// D5) est reservee au daemon.
pub fn scan_claude_measurements(projects_dir: &Path) -> Vec<Measurement> {
    let mut acc: MeasAcc = HashMap::new();
    for p in claude_transcript_files(projects_dir) {
        if let Ok(content) = std::fs::read_to_string(&p) {
            fold_file_measure(&mut acc, &content);
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
        let v = finalize(acc, &CwdAcc::new(), 2);
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

    // ---------------- D1 : marche recursive (claude_transcript_files) ----------------

    /// Racine des fixtures Claude du repo (alpha a des sous-agents, beta a des doublons,
    /// work est une racine de portefeuille) — memes fixtures que `src-tauri/src/history.rs`.
    fn mock_claude_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../specs/mock/claude_projects")
    }

    #[test]
    fn claude_transcript_files_trouve_session_principale_et_sous_agent() {
        let files = claude_transcript_files(&mock_claude_dir());
        let names: Vec<String> = files
            .iter()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        assert!(
            names.iter().any(|n| n.ends_with("-w-alpha/session-alpha.jsonl")),
            "transcript principal alpha manquant : {names:?}"
        );
        assert!(
            names
                .iter()
                .any(|n| n.ends_with("session-alpha/subagents/agent-1.jsonl")),
            "transcript de sous-agent manquant (marche non recursive avant L0) : {names:?}"
        );
    }

    #[test]
    fn claude_transcript_files_dossier_absent_liste_vide() {
        assert!(claude_transcript_files(Path::new("/dossier/inexistant/xyz")).is_empty());
    }

    // ---------------- D2 : deduplication par fichier (message.id) ----------------

    #[test]
    fn dedup_lines_garde_la_derniere_occurrence_et_conserve_les_lignes_sans_id() {
        let content = concat!(
            r#"{"type":"assistant","cwd":"/w/p","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":1}}}"#,
            "\n",
            r#"{"type":"assistant","cwd":"/w/p","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":1}}}"#,
            "\n",
            r#"{"type":"assistant","cwd":"/w/p","message":{"usage":{"input_tokens":5,"output_tokens":0}}}"#, // sans id
        );
        let kept = dedup_lines_by_message_id(content);
        assert_eq!(kept.len(), 2); // 1 occurrence de m1 (la derniere) + la ligne sans id
    }

    #[test]
    fn fold_file_economy_message_id_triple_produit_le_tiers_du_total_naif() {
        let content = concat!(
            r#"{"type":"assistant","cwd":"/w/p","message":{"id":"dup","usage":{"input_tokens":900,"output_tokens":300}}}"#,
            "\n",
            r#"{"type":"assistant","cwd":"/w/p","message":{"id":"dup","usage":{"input_tokens":900,"output_tokens":300}}}"#,
            "\n",
            r#"{"type":"assistant","cwd":"/w/p","message":{"id":"dup","usage":{"input_tokens":900,"output_tokens":300}}}"#,
        );
        let mut naive = Acc::new();
        for line in content.lines() {
            fold_line(&mut naive, line);
        }
        assert_eq!(naive["p"].0 + naive["p"].1, 3600); // naif : 3 x (900+300)

        let mut deduped = Acc::new();
        fold_file_economy(&mut deduped, content);
        assert_eq!(deduped["p"].0 + deduped["p"].1, 1200); // deduplique : 1 x (900+300) = le tiers
    }

    #[test]
    fn fold_file_measure_deduplique_aussi() {
        let content = concat!(
            r#"{"type":"assistant","cwd":"/w/p","message":{"id":"dup","usage":{"input_tokens":100,"output_tokens":50}}}"#,
            "\n",
            r#"{"type":"assistant","cwd":"/w/p","message":{"id":"dup","usage":{"input_tokens":100,"output_tokens":50}}}"#,
        );
        let mut acc = MeasAcc::new();
        fold_file_measure(&mut acc, content);
        assert_eq!(acc[&("p".to_string(), Agent::Coordinator)].used(), 150); // 1 seule occurrence
    }

    #[test]
    fn fixture_reelle_beta_message_id_triple_est_deduplique() {
        // session-beta.jsonl : ligne 1 (2000+800=2800) + ligne 3 (50+10 input, 20 output = 80)
        // = 2880 sans id, PLUS 3 lignes identiques "msg_dup_1" (900 input + 300 output). Sans
        // dedup : 2880 + 3*1200 = 6480. Avec dedup (D2, garde la derniere) : 2880 + 1200 = 4080.
        let dir = mock_claude_dir();
        let economy = scan_projects_dir(&dir, 20);
        let beta = economy.iter().find(|e| e.project == "beta").expect("beta");
        assert_eq!(beta.input + beta.output, 4080, "{beta:?}");
    }

    // ---------------- D4 : seau "hors projet" + infobulle cwd ----------------

    #[test]
    fn bucket_project_regroupe_les_racines_de_portefeuille() {
        assert_eq!(bucket_project("work".into()), OUT_OF_PROJECT_BUCKET);
        assert_eq!(bucket_project("Desktop".into()), OUT_OF_PROJECT_BUCKET);
        assert_eq!(bucket_project("iaka-demo".into()), "iaka-demo");
    }

    #[test]
    fn fixture_reelle_work_tombe_dans_hors_projet() {
        let dir = mock_claude_dir();
        let economy = scan_projects_dir(&dir, 20);
        assert!(
            economy.iter().all(|e| e.project != "work"),
            "la racine de portefeuille 'work' ne doit jamais apparaitre telle quelle"
        );
        let bucket = economy
            .iter()
            .find(|e| e.project == OUT_OF_PROJECT_BUCKET)
            .expect("seau hors projet attendu");
        assert_eq!(bucket.input + bucket.output, 80); // 70 + 10 (fixture -w-work)
        assert_eq!(bucket.example_cwd, "/w/work");
    }

    #[test]
    fn example_cwd_for_projet_normal_et_seau_hors_projet() {
        let mut examples = CwdAcc::new();
        examples.insert("iaka-demo".into(), "/x/iaka-demo".into());
        examples.insert("work".into(), "/z/work".into());
        examples.insert("Desktop".into(), "/a/Desktop".into());
        assert_eq!(example_cwd_for("iaka-demo", &examples), "/x/iaka-demo");
        // Seau hors projet -> le plus petit cwd parmi les racines connues (deterministe).
        assert_eq!(example_cwd_for(OUT_OF_PROJECT_BUCKET, &examples), "/a/Desktop");
        assert_eq!(example_cwd_for("inconnu", &examples), "");
    }

    // ---------------- Contre-epreuve du diagnostic (instruction § Comportement attendu) ----------------

    /// Reproduit EXACTEMENT le comportement d'AVANT L0 (marche non recursive, sans dedup) :
    /// c'est la contre-epreuve exigee par l'instruction — desactiver recursion ET deduplication
    /// doit reproduire ce que publiait le daemon avant ce lot.
    fn scan_pre_l0(projects_dir: &Path) -> u64 {
        let mut acc: Acc = HashMap::new();
        let dirs = match std::fs::read_dir(projects_dir) {
            Ok(d) => d,
            Err(_) => return 0,
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
        acc.values().map(|(i, o, _, _)| i + o).sum()
    }

    #[test]
    fn contre_epreuve_pre_l0_reproduit_exactement_le_modele_du_defaut_avant_ce_lot() {
        // `scan_pre_l0` REJOUE le code de production AVANT ce lot (marche non recursive, sans
        // dedup) : c'est la contre-epreuve de l'instruction. Sur les VRAIES donnees du decideur,
        // le volume manque des sous-agents (69,5 % du total reel) l'emporte tres largement sur
        // l'inflation des doublons (facteur ~1,93) : pre-L0 (~4,96 Md) est BIEN INFERIEUR au total
        // L0 (~9,2 Md) -- cf. criteres chiffres de l'instruction, verifies manuellement sur
        // `~/.claude/projects` (hors fixtures, cf. message de remise). Sur CETTE fixture reduite,
        // le rapport de grandeur est inverse (le fold beta triple pese plus que le seul fichier
        // de sous-agent alpha) : ce test fige donc les deux valeurs EXACTES plutot que leur ordre,
        // pour ne pas laisser croire qu'un fixture miniature a la meme direction que les vraies
        // donnees.
        let dir = mock_claude_dir();
        let pre_l0_total = scan_pre_l0(&dir);
        let l0_total: u64 = scan_projects_dir(&dir, 20)
            .iter()
            .map(|e| e.input + e.output)
            .sum();
        assert_eq!(pre_l0_total, 9210, "modele du defaut (avant L0) change de valeur sur fixture");
        assert_eq!(l0_total, 7360, "total L0 (recursif + deduplique) change de valeur sur fixture");
    }
}
