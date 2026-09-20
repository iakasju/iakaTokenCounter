//! agents — agents Claude Code « en cours » (feature-agents-en-cours.md) : roster de sprites
//! (D4), lecture des sidecars `agent-<id>.meta.json` (D1), liveness par mtime (D2), construction
//! de l'arbre de delegation (`parentAgentId`, exact a toute profondeur, D7), instantane + commande
//! + watcher (D3).
//!
//! Module ADDITIF et DISJOINT du reste de l'app (D8) : aucun fichier existant reecrit ici. Les
//! points de contact avec le lot L1 (memoire historique) sont volontairement minimes : un champ
//! ajoute en fin d'`AppState`, une commande ajoutee en fin d'`invoke_handler`, un evenement Tauri
//! separe (`tray://agents`, distinct de `tray://state`). Seule modification hors de ce module :
//! `project_of`/`bucket_project` d'`iakatc-core` passes de `pub(crate)` a `pub` (visibilite seule,
//! comportement inchange) pour resoudre le nom de projet d'une session (D7) sans dupliquer cette
//! logique deja ecrite et testee.
//!
//! Toute la logique (roster, parsing, liveness, arbre, tris, bornes) vit ICI : `render.ts` ne fait
//! que peindre le modele deja resolu (D9) — le projet n'a aucun harnais de test JS.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

use iakatc_core::measure::claude::{
    bucket_project, claude_projects_dir, claude_transcript_files, project_of,
};

use crate::state::AppState;

/// Nom d'evenement pousse a la webview a chaque changement d'instantane agents (D3). Separe de
/// `tray://state` (D8) : une panne du chemin quota n'eteint pas les sprites, et reciproquement.
pub const AGENTS_EVENT: &str = "tray://agents";

/// Cadence du watcher dedie (D3).
const TICK: Duration = Duration::from_secs(5);

/// Bornes d'affichage de la popover (D7) : au-dela, un marqueur « +N ».
const MAX_SESSIONS: usize = 6;
const MAX_CHILDREN: usize = 8;

/// Plafond de lecture de la premiere ligne d'un transcript (D7 : lecture bornee, uniquement pour
/// les sessions vivantes).
const FIRST_LINE_CAP: usize = 1024 * 1024; // 1 MiB

// ============================== D4 : roster et palette (etape 1) ==============================

/// Sprite resolu d'un agent : lettre + couleurs (fond/texte), deja pretes a peindre (D9). Rendu
/// DOM/CSS cote webview (carre arrondi), jamais une image (D7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sprite {
    pub letter: char,
    pub bg: &'static str,
    pub fg: &'static str,
}

/// Table figee `agentType -> (lettre, fond, texte)` (D4). Codee en dur : l'app reste autonome,
/// aucune lecture de `~/work/iakaframe` a l'execution. Source : frontmatter `pastille` de
/// `~/work/iakaframe/library/personas/*.md`, deja resolu en hex par le cadrage (§ Sources).
const ROSTER: &[(&str, char, &str, &str)] = &[
    ("odin", 'O', "#FFD60A", "#0B0D12"),
    ("aragorn", 'A', "#FF9F0A", "#0B0D12"),
    ("gandalf", 'G', "#0A84FF", "#FFFFFF"),
    ("gimli", 'G', "#FF3B30", "#FFFFFF"),
    ("legolas", 'L', "#FF3B30", "#FFFFFF"),
    ("helm", 'H', "#BF5AF2", "#FFFFFF"),
    ("loki", 'L', "#FF9F0A", "#0B0D12"),
    ("nathalie", 'N', "#FF9F0A", "#0B0D12"),
    ("feanor", 'F', "#FF9F0A", "#0B0D12"),
];

/// Fond/texte du repli « hors roster » (gris neutre, D4) : jamais deguise en persona de la frame.
const FALLBACK_BG: &str = "#6F6F78";
const FALLBACK_FG: &str = "#FFFFFF";

/// Resout le sprite d'un `agentType` (insensible a la casse). Type du roster -> table D4 ; type
/// hors roster -> premiere lettre du type (majuscule) sur fond gris ; type vide (sidecar
/// absent/invalide, D1) -> `?` sur fond gris. Jamais de panique, jamais de faux-persona.
pub fn sprite_for(agent_type: &str) -> Sprite {
    let lower = agent_type.to_ascii_lowercase();
    if let Some(&(_, letter, bg, fg)) = ROSTER.iter().find(|(t, ..)| *t == lower) {
        return Sprite { letter, bg, fg };
    }
    let letter = agent_type
        .chars()
        .next()
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or('?');
    Sprite {
        letter,
        bg: FALLBACK_BG,
        fg: FALLBACK_FG,
    }
}

/// Sprite conventionnel du coordinateur de session (D7/D8) : « un coordinateur *est* un agent —
/// Odin, cf. CLAUDE.md global ». Le coordinateur (session Claude Code de premier niveau) ne porte
/// aucun sidecar de persona ; la methode le designe par convention comme Odin (le portefeuille
/// toujours actif, cf. skill `iakastart`) — coherent avec l'exemple du decideur (`O` en tete).
pub fn coordinator_sprite() -> Sprite {
    sprite_for("odin")
}

// ========================= D1 (etape 2) : lecture du sidecar meta.json =========================

/// Identite + parente d'un sous-agent, lue depuis son sidecar `agent-<id>.meta.json` (D1).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentMeta {
    /// `agentType` (nom du persona). Vide si absent/illisible/invalide -> type inconnu (D4).
    pub agent_type: String,
    pub description: Option<String>,
    /// Absent -> le parent est le coordinateur de la session (D1).
    pub parent_agent_id: Option<String>,
    pub spawn_depth: u32,
}

/// Forme brute du JSON du sidecar (camelCase, tous les champs par defaut) : un champ manquant
/// retombe sur sa valeur par defaut plutot que de faire echouer tout le parsing (D1).
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawAgentMeta {
    #[serde(default)]
    agent_type: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    parent_agent_id: Option<String>,
    #[serde(default)]
    spawn_depth: u32,
}

impl From<RawAgentMeta> for AgentMeta {
    fn from(r: RawAgentMeta) -> Self {
        AgentMeta {
            agent_type: r.agent_type,
            description: r.description,
            parent_agent_id: r.parent_agent_id,
            spawn_depth: r.spawn_depth,
        }
    }
}

/// Parse le CONTENU d'un sidecar (pur, testable). JSON invalide -> `None`. Un objet JSON valide
/// mais partiel (champs manquants) produit un `AgentMeta` avec les valeurs par defaut (D1).
pub fn parse_agent_meta(content: &str) -> Option<AgentMeta> {
    serde_json::from_str::<RawAgentMeta>(content)
        .ok()
        .map(AgentMeta::from)
}

/// Lit et parse le sidecar sur disque. Fichier absent/illisible/JSON invalide -> `None` (le niveau
/// appelant retombe alors sur `AgentMeta::default()`, jamais une panique, D1).
fn read_agent_meta(path: &Path) -> Option<AgentMeta> {
    let content = std::fs::read_to_string(path).ok()?;
    parse_agent_meta(&content)
}

// ============================ D2 (etape 3) : liveness par mtime ============================

/// Un agent est vivant ssi `now - mtime <= n` (D2, N = 90 s par defaut, `IAKATC_LIVENESS_SECS`).
/// Defensif face a une horloge en avance (mtime futur, diff negative) : traite comme vivant.
pub fn is_live(now: i64, mtime: i64, n: i64) -> bool {
    now - mtime <= n
}

fn epoch_of(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn mtime_of(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .map(epoch_of)
}

fn created_of(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .and_then(|m| m.created())
        .ok()
        .map(epoch_of)
}

// ===================== Decouverte des fichiers de session (D1, etape 3) =====================

/// Un transcript de sous-agent identifie structurellement : `<sid>/subagents/agent-<id>.jsonl`
/// (D1). Extrait l'identifiant `<id>` du nom de fichier. `None` si le chemin ne correspond pas a
/// ce patron (ex. un futur `journal.jsonl` d'orchestration : pas un agent, ignore — D1).
fn subagent_id(path: &Path) -> Option<String> {
    let parent = path.parent()?;
    if parent.file_name()?.to_str()? != "subagents" {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    stem.strip_prefix("agent-").map(str::to_string)
}

/// Cle de regroupement d'une session : dossier de projet echappe + identifiant de session.
type SessionKey = (PathBuf, String);

struct RawSubagent {
    agent_id: String,
    transcript_path: PathBuf,
}

#[derive(Default)]
struct RawSession {
    coordinator_transcript: Option<PathBuf>,
    subagents: Vec<RawSubagent>,
}

/// Classe tous les transcripts sous `projects_dir` par session (D1) : `<sid>.jsonl` = transcript
/// du coordinateur ; `<sid>/subagents/agent-<id>.jsonl` = transcript de sous-agent. Reutilise
/// `claude_transcript_files` (deja ecrit, D1 verite-des-chiffres) : marche recursive, aucun
/// contenu lu ici (D3 : seuls des `metadata()` sont poses plus loin).
fn group_sessions(projects_dir: &Path) -> HashMap<SessionKey, RawSession> {
    let mut sessions: HashMap<SessionKey, RawSession> = HashMap::new();
    for path in claude_transcript_files(projects_dir) {
        if let Some(agent_id) = subagent_id(&path) {
            // .../<escaped>/<sid>/subagents/agent-<id>.jsonl
            let Some(subagents_dir) = path.parent() else {
                continue;
            };
            let Some(sid_dir) = subagents_dir.parent() else {
                continue;
            };
            let Some(escaped_dir) = sid_dir.parent() else {
                continue;
            };
            let Some(sid) = sid_dir.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let key = (escaped_dir.to_path_buf(), sid.to_string());
            sessions
                .entry(key)
                .or_default()
                .subagents
                .push(RawSubagent {
                    agent_id,
                    transcript_path: path,
                });
        } else {
            // .../<escaped>/<sid>.jsonl
            let Some(escaped_dir) = path.parent() else {
                continue;
            };
            let Some(sid) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let key = (escaped_dir.to_path_buf(), sid.to_string());
            sessions.entry(key).or_default().coordinator_transcript = Some(path);
        }
    }
    sessions
}

// ========================== Etape 4 : arbre de delegation (pur, teste) ==========================

/// Noeud d'agent deja resolu pour l'affichage (D7/D9) : sprite + infobulle + enfants imbriques.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentNode {
    pub sprite: Sprite,
    pub tooltip: String,
    pub children: Vec<AgentNode>,
    /// Nombre d'agents au-dela de la borne (D7, 8 enfants max) — `None` si aucun debordement ;
    /// porte par un noeud marqueur synthetique ("+N") plutot que par un champ du parent.
    pub overflow: Option<u32>,
}

/// Une session vivante affichee dans la popover (D7) : libelle de projet + racine (coordinateur).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionNode {
    pub project: String,
    pub coordinator: AgentNode,
}

/// Sous-agent resolu (meta + liveness + horodatages), pret pour la construction de l'arbre.
#[derive(Debug, Clone)]
struct ResolvedSubagent {
    agent_id: String,
    meta: AgentMeta,
    live: bool,
    mtime: Option<i64>,
    created: Option<i64>,
}

/// Resout un `RawSubagent` (lecture du sidecar + mtime + liveness). Defensif (D1) : sidecar
/// absent/illisible/invalide -> `AgentMeta::default()` (type inconnu), jamais ignore : c'est le
/// `*.jsonl` de `subagents/` lui-meme (patron `agent-<id>.jsonl`) qui fait foi qu'il y a un agent.
fn resolve_subagent(raw: &RawSubagent, now: i64, liveness_secs: i64) -> ResolvedSubagent {
    let sidecar = raw.transcript_path.with_extension("meta.json");
    let meta = read_agent_meta(&sidecar).unwrap_or_default();
    let mtime = mtime_of(&raw.transcript_path);
    let live = mtime.is_some_and(|m| is_live(now, m, liveness_secs));
    let created = created_of(&raw.transcript_path);
    ResolvedSubagent {
        agent_id: raw.agent_id.clone(),
        meta,
        live,
        mtime,
        created,
    }
}

/// Construit le noeud de session affiche (D7) + son activite (pour le tri, D7), ou `None` si
/// aucun agent n'y est vivant (D2 : « session sans agent vivant -> absente »).
fn build_session_node(
    escaped_dir: &Path,
    raw: &RawSession,
    now: i64,
    liveness_secs: i64,
) -> Option<(SessionNode, i64)> {
    let coordinator_mtime = raw.coordinator_transcript.as_deref().and_then(mtime_of);
    let coordinator_own_live =
        coordinator_mtime.is_some_and(|m| is_live(now, m, liveness_secs));

    let resolved: Vec<ResolvedSubagent> = raw
        .subagents
        .iter()
        .map(|s| resolve_subagent(s, now, liveness_secs))
        .collect();

    let any_descendant_live = resolved.iter().any(|s| s.live);
    if !coordinator_own_live && !any_descendant_live {
        return None; // D2 : coordinateur froid, aucun descendant vivant -> session absente.
    }

    let by_id: HashMap<String, &ResolvedSubagent> = resolved
        .iter()
        .map(|s| (s.agent_id.clone(), s))
        .collect();

    // Parent d'affichage de chaque agent VIVANT : on remonte la chaine `parentAgentId` jusqu'au
    // premier ancetre vivant ; a defaut (chaine epuisee ou ancetre inconnu), coordinateur (`None`).
    // `parentAgentId` reste la verite de la delegation (D1) ; seule la NESTING d'AFFICHAGE saute
    // les ancetres morts (D7 : « parent termine, enfant encore vivant »).
    let mut display_parent: HashMap<String, Option<String>> = HashMap::new();
    for s in resolved.iter().filter(|s| s.live) {
        let mut current = s.meta.parent_agent_id.clone();
        let mut seen: HashSet<String> = HashSet::new();
        let mut found: Option<String> = None;
        while let Some(pid) = current {
            if !seen.insert(pid.clone()) {
                break; // cycle defensif (ne devrait jamais arriver) -> rattache au coordinateur.
            }
            match by_id.get(&pid) {
                Some(p) if p.live => {
                    found = Some(pid);
                    break;
                }
                Some(p) => current = p.meta.parent_agent_id.clone(), // remonte encore
                None => break, // ancetre inconnu -> coordinateur
            }
        }
        display_parent.insert(s.agent_id.clone(), found);
    }

    let mut children_of: HashMap<Option<String>, Vec<String>> = HashMap::new();
    for s in resolved.iter().filter(|s| s.live) {
        let parent = display_parent.get(&s.agent_id).cloned().flatten();
        children_of.entry(parent).or_default().push(s.agent_id.clone());
    }

    let coordinator_children = build_children(None, &children_of, &by_id, &display_parent, now);
    let elapsed = coordinator_mtime.map(|m| (now - m).max(0)).unwrap_or(0);
    let coordinator = AgentNode {
        sprite: coordinator_sprite(),
        tooltip: format!("odin (coordinateur) — actif il y a {elapsed} s"),
        children: coordinator_children,
        overflow: None,
    };

    let project = project_label_for_session(escaped_dir, raw.coordinator_transcript.as_deref());

    let activity = std::iter::once(coordinator_mtime.unwrap_or(i64::MIN))
        .chain(resolved.iter().filter_map(|s| s.mtime))
        .max()
        .unwrap_or(i64::MIN);

    Some((SessionNode { project, coordinator }, activity))
}

/// Construit les enfants (tries, bornes a 8 + marqueur `+N`, D7) d'un parent d'affichage donne
/// (`None` = coordinateur). Recursif : chaque enfant reconstruit a son tour ses propres enfants.
fn build_children(
    parent: Option<String>,
    children_of: &HashMap<Option<String>, Vec<String>>,
    by_id: &HashMap<String, &ResolvedSubagent>,
    display_parent: &HashMap<String, Option<String>>,
    now: i64,
) -> Vec<AgentNode> {
    let Some(ids) = children_of.get(&parent) else {
        return Vec::new();
    };
    let mut kids: Vec<&ResolvedSubagent> = ids
        .iter()
        .filter_map(|id| by_id.get(id).copied())
        .collect();
    // Tri D7 : date de creation du transcript croissante (ordre de delegation), repli agentId
    // lexicographique (creation indisponible ou egale) pour rester deterministe.
    kids.sort_by(|a, b| {
        a.created
            .unwrap_or(i64::MAX)
            .cmp(&b.created.unwrap_or(i64::MAX))
            .then_with(|| a.agent_id.cmp(&b.agent_id))
    });
    let overflow = kids.len().saturating_sub(MAX_CHILDREN);
    let mut nodes: Vec<AgentNode> = kids
        .into_iter()
        .take(MAX_CHILDREN)
        .map(|s| {
            let sprite = sprite_for(&s.meta.agent_type);
            let elapsed = s.mtime.map(|m| (now - m).max(0)).unwrap_or(0);
            let persona = if s.meta.agent_type.is_empty() {
                "inconnu"
            } else {
                s.meta.agent_type.as_str()
            };
            let desc = s.meta.description.as_deref().unwrap_or("");
            let mut tooltip = if desc.is_empty() {
                format!("{persona} — actif il y a {elapsed} s")
            } else {
                format!("{persona} — {desc} — actif il y a {elapsed} s")
            };
            // Parent reel different du parent d'affichage (remontee, D7) : le mentionner.
            let display_p = display_parent.get(&s.agent_id).cloned().flatten();
            if s.meta.parent_agent_id != display_p {
                let real = s
                    .meta
                    .parent_agent_id
                    .as_ref()
                    .and_then(|pid| by_id.get(pid))
                    .map(|p| {
                        if p.meta.agent_type.is_empty() {
                            "inconnu".to_string()
                        } else {
                            p.meta.agent_type.clone()
                        }
                    })
                    .unwrap_or_else(|| "coordinateur".to_string());
                tooltip.push_str(&format!(" (parent reel : {real}, termine)"));
            }
            let children = build_children(
                Some(s.agent_id.clone()),
                children_of,
                by_id,
                display_parent,
                now,
            );
            AgentNode {
                sprite,
                tooltip,
                children,
                overflow: None,
            }
        })
        .collect();
    if overflow > 0 {
        nodes.push(AgentNode {
            sprite: Sprite {
                letter: '+',
                bg: FALLBACK_BG,
                fg: FALLBACK_FG,
            },
            tooltip: format!("+{overflow} agent(s) supplementaire(s)"),
            children: Vec::new(),
            overflow: Some(overflow as u32),
        });
    }
    nodes
}

// ============================ Etape 5 : label de projet ============================

/// Lit la PREMIERE ligne (plafonnee a 1 MiB) d'un transcript et en extrait le `cwd`, projete via
/// `project_of` + `bucket_project` (reutilise iakatc-core, D7 : meme nom de projet que partout
/// ailleurs dans l'app, seau « hors projet » compris). Repli : nom du dossier de projet echappe,
/// tel quel.
fn project_label_for_session(escaped_dir: &Path, coordinator_transcript: Option<&Path>) -> String {
    let fallback = || {
        escaped_dir
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    };
    let Some(path) = coordinator_transcript else {
        return fallback();
    };
    let Some(first_line) = read_first_line_capped(path, FIRST_LINE_CAP) else {
        return fallback();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&first_line) else {
        return fallback();
    };
    let Some(cwd) = v.get("cwd").and_then(|c| c.as_str()) else {
        return fallback();
    };
    match project_of(cwd) {
        Some(p) => bucket_project(p),
        None => fallback(),
    }
}

/// Lit au plus `cap` octets puis s'arrete a la premiere ligne complete rencontree. `None` si le
/// fichier est illisible ou vide (defensif).
fn read_first_line_capped(path: &Path, cap: usize) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; cap];
    let n = f.read(&mut buf).ok()?;
    if n == 0 {
        return None;
    }
    buf.truncate(n);
    let text = String::from_utf8_lossy(&buf);
    let line = text.lines().next()?;
    if line.is_empty() {
        return None;
    }
    Some(line.to_string())
}

// ================= Etape 6 : instantane + commande + watcher =================

/// Instantane complet pousse a la webview (D9 : modele deja resolu, camelCase).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsSnapshot {
    /// Effectif total = tous les agents vivants, coordinateurs compris (D8 : « un coordinateur
    /// *est* un agent »). Meme ensemble que la popover, overflow inclus (pas seulement les
    /// sprites effectivement peints apres troncature d'affichage).
    pub count: u32,
    pub sessions: Vec<SessionNode>,
    /// Nombre de sessions vivantes au-dela de la borne d'affichage (D7, 6 max).
    pub overflow_sessions: Option<u32>,
}

/// Compte les agents reellement vivants d'un noeud (recursif) : la racine + chaque enfant, un
/// marqueur de debordement comptant pour son `overflow` plutot que pour 1 (D8 : l'effectif doit
/// rester vrai meme au-dela de la troncature d'affichage, D7).
fn count_live_agents(node: &AgentNode) -> u32 {
    let mut total = 1;
    for child in &node.children {
        total += child.overflow.unwrap_or_else(|| count_live_agents(child));
    }
    total
}

/// Construit l'instantane courant (pur, sauf l'horloge/le disque passes en parametre) : marche +
/// resolution + tri + bornes (D7). `now`/`liveness_secs` explicites -> testable sans horloge
/// murale ni variable d'environnement.
pub fn snapshot_at(projects_dir: &Path, now: i64, liveness_secs: i64) -> AgentsSnapshot {
    let raw_sessions = group_sessions(projects_dir);
    let mut with_activity: Vec<(i64, SessionNode)> = Vec::new();
    let mut count: u32 = 0;

    for (key, raw) in &raw_sessions {
        let Some((node, activity)) = build_session_node(&key.0, raw, now, liveness_secs) else {
            continue;
        };
        count += count_live_agents(&node.coordinator);
        with_activity.push((activity, node));
    }

    // Tri par activite decroissante (D7). `key.1` (identifiant de session) sert de repli
    // deterministe en cas d'egalite d'activite (non specifie par l'instruction, mais requis pour
    // un ordre stable d'un tick a l'autre — condition de l'emission-si-changement, D3).
    with_activity.sort_by_key(|(activity, _)| std::cmp::Reverse(*activity));

    let overflow = with_activity.len().saturating_sub(MAX_SESSIONS);
    let sessions: Vec<SessionNode> = with_activity
        .into_iter()
        .take(MAX_SESSIONS)
        .map(|(_, n)| n)
        .collect();

    AgentsSnapshot {
        count,
        sessions,
        overflow_sessions: if overflow > 0 {
            Some(overflow as u32)
        } else {
            None
        },
    }
}

/// Instantane courant, horloge murale + seuil configure (`IAKATC_LIVENESS_SECS`, config.rs).
/// Repertoire des projets introuvable (HOME absent) -> instantane vide (defensif).
fn current_snapshot() -> AgentsSnapshot {
    let now = crate::memory::now_secs(); // reutilise l'horloge deja ecrite (meme patron, D3).
    let liveness = crate::config::liveness_secs_from_env() as i64;
    match claude_projects_dir() {
        Some(dir) => snapshot_at(&dir, now, liveness),
        None => AgentsSnapshot::default(),
    }
}

/// Commande : instantane des agents en cours a l'ouverture de la popover (D6/etape 6). Lit
/// l'instantane deja pose par le watcher dans `AppState` (meme patron que `get_memory_history`) :
/// pas de re-scan disque synchrone au moment de l'appel.
#[tauri::command]
pub fn get_running_agents(state: tauri::State<'_, AppState>) -> AgentsSnapshot {
    state.running_agents.lock().unwrap().clone()
}

/// Lance le watcher dedie dans un thread detache (patron `memory::start_sampler`, D3). Tick 5 s,
/// metadonnees seules (aucun contenu de transcript relu, hormis les sidecars ~200 o et la premiere
/// ligne des sessions vivantes, D3) ; emet `tray://agents` UNIQUEMENT si l'instantane a change
/// (meme discipline que `ReservoirStore::apply_message`) et recompose l'icone (compteur, D6).
pub fn start_watcher(app: AppHandle) {
    std::thread::spawn(move || run_watcher(app));
}

fn run_watcher(app: AppHandle) {
    let mut last: Option<AgentsSnapshot> = None;
    loop {
        let snap = current_snapshot();
        {
            let state = app.state::<AppState>();
            *state.running_agents.lock().unwrap() = snap.clone();
        }
        if last.as_ref() != Some(&snap) {
            let _ = app.emit(AGENTS_EVENT, &snap);
            // Recompose l'icone avec le nouvel effectif, sur la base du pire compte courant
            // (D6/D8) : le chemin agents ne touche pas au store de reservoirs, il le relit.
            let cards = app.state::<AppState>().store.lock().unwrap().cards();
            crate::tray::update_icon(&app, &cards, snap.count);
            last = Some(snap);
        }
        std::thread::sleep(TICK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // ---------------------------- Etape 1 : roster (D4) ----------------------------

    #[test]
    fn sprite_for_rend_les_neuf_personas_du_roster() {
        assert_eq!(sprite_for("odin"), Sprite { letter: 'O', bg: "#FFD60A", fg: "#0B0D12" });
        assert_eq!(sprite_for("aragorn"), Sprite { letter: 'A', bg: "#FF9F0A", fg: "#0B0D12" });
        assert_eq!(sprite_for("gandalf"), Sprite { letter: 'G', bg: "#0A84FF", fg: "#FFFFFF" });
        assert_eq!(sprite_for("gimli"), Sprite { letter: 'G', bg: "#FF3B30", fg: "#FFFFFF" });
        assert_eq!(sprite_for("legolas"), Sprite { letter: 'L', bg: "#FF3B30", fg: "#FFFFFF" });
        assert_eq!(sprite_for("helm"), Sprite { letter: 'H', bg: "#BF5AF2", fg: "#FFFFFF" });
        assert_eq!(sprite_for("loki"), Sprite { letter: 'L', bg: "#FF9F0A", fg: "#0B0D12" });
        assert_eq!(sprite_for("nathalie"), Sprite { letter: 'N', bg: "#FF9F0A", fg: "#0B0D12" });
        assert_eq!(sprite_for("feanor"), Sprite { letter: 'F', bg: "#FF9F0A", fg: "#0B0D12" });
    }

    #[test]
    fn sprite_for_type_hors_roster_prend_sa_propre_lettre_en_gris() {
        let s = sprite_for("claude-code-guide");
        assert_eq!(s, Sprite { letter: 'C', bg: FALLBACK_BG, fg: FALLBACK_FG });
        let s2 = sprite_for("general-purpose");
        assert_eq!(s2.letter, 'G');
        assert_eq!(s2.bg, FALLBACK_BG);
    }

    #[test]
    fn sprite_for_type_vide_ne_panique_pas() {
        let s = sprite_for("");
        assert_eq!(s, Sprite { letter: '?', bg: FALLBACK_BG, fg: FALLBACK_FG });
    }

    #[test]
    fn sprite_for_est_insensible_a_la_casse() {
        assert_eq!(sprite_for("GIMLI"), sprite_for("gimli"));
    }

    #[test]
    fn roster_couples_lettre_fond_deux_a_deux_distincts() {
        // Contrainte a tester, pas un hasard a subir (D4) : si la frame gagne un persona qui
        // collisionne avec un couple existant, ce test doit peter.
        let mut seen: HashSet<(char, &'static str)> = HashSet::new();
        for &(agent_type, letter, bg, _) in ROSTER {
            let key = (letter, bg);
            assert!(
                seen.insert(key),
                "couple (lettre, fond) duplique pour {agent_type} : {key:?}"
            );
        }
        assert_eq!(seen.len(), ROSTER.len());
    }

    #[test]
    fn coordinator_sprite_est_odin() {
        assert_eq!(coordinator_sprite(), sprite_for("odin"));
        assert_eq!(coordinator_sprite().letter, 'O');
    }

    // ---------------------------- Etape 2 : sidecar meta.json (D1) ----------------------------

    #[test]
    fn parse_agent_meta_profondeur_1_sans_parent_agent_id() {
        let json = r#"{"agentType":"gandalf","description":"Cadrer la feature agents en cours",
 "toolUseId":"toolu_013irH9ZFSiCUTwm3qKK8YXT","spawnDepth":1,
 "requestShape":"background","requestNonInteractive":true}"#;
        let meta = parse_agent_meta(json).expect("json valide");
        assert_eq!(meta.agent_type, "gandalf");
        assert_eq!(meta.description.as_deref(), Some("Cadrer la feature agents en cours"));
        assert_eq!(meta.parent_agent_id, None);
        assert_eq!(meta.spawn_depth, 1);
    }

    #[test]
    fn parse_agent_meta_profondeur_2_avec_parent_agent_id() {
        let json = r#"{"agentType":"loki","description":"Stand down Loki","toolUseId":"toolu_01ARZQBw",
 "parentAgentId":"a32f41f2eb6ff10ea","spawnDepth":2,
 "requestShape":"background","requestNonInteractive":true}"#;
        let meta = parse_agent_meta(json).expect("json valide");
        assert_eq!(meta.agent_type, "loki");
        assert_eq!(meta.parent_agent_id.as_deref(), Some("a32f41f2eb6ff10ea"));
        assert_eq!(meta.spawn_depth, 2);
    }

    #[test]
    fn parse_agent_meta_json_invalide_est_none() {
        assert_eq!(parse_agent_meta("pas du json"), None);
        assert_eq!(parse_agent_meta(""), None);
        assert_eq!(parse_agent_meta("{"), None);
    }

    #[test]
    fn parse_agent_meta_champ_manquant_retombe_sur_le_defaut() {
        let meta = parse_agent_meta(r#"{"agentType":"gimli"}"#).expect("json valide");
        assert_eq!(meta.agent_type, "gimli");
        assert_eq!(meta.description, None);
        assert_eq!(meta.parent_agent_id, None);
        assert_eq!(meta.spawn_depth, 0);
        // Objet vide : tout par defaut, mais toujours `Some` (pas un JSON invalide).
        let empty = parse_agent_meta("{}").expect("objet vide valide");
        assert_eq!(empty.agent_type, "");
    }

    #[test]
    fn read_agent_meta_fichier_absent_est_none() {
        assert_eq!(read_agent_meta(Path::new("/dossier/inexistant/agent-x.meta.json")), None);
    }

    // ---------------------------- Etape 3 : liveness (D2) ----------------------------

    #[test]
    fn is_live_bornes_0_89_90_91() {
        let now = 1_000_000i64;
        assert!(is_live(now, now, 90)); // 0 s
        assert!(is_live(now, now - 89, 90));
        assert!(is_live(now, now - 90, 90)); // borne haute incluse
        assert!(!is_live(now, now - 91, 90));
    }

    #[test]
    fn is_live_horloge_en_avance_reste_vivant() {
        // mtime "futur" (skew d'horloge) -> diff negative -> vivant (defensif).
        assert!(is_live(1_000_000, 1_000_100, 90));
    }

    /// Dossier temporaire unique pour un test de decouverte de sessions.
    fn tmp_projects_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("itc-agents-{name}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Ecrit un fichier avec un contenu et un mtime explicite (evite les sleeps de test : les
    /// scenarios de liveness ont besoin de fichiers "vieux de N secondes" de facon deterministe).
    fn write_with_mtime(path: &Path, content: &str, age_secs: i64) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
        let mtime = SystemTime::now() - Duration::from_secs(age_secs.max(0) as u64);
        let f = fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_times(std::fs::FileTimes::new().set_modified(mtime)).unwrap();
    }

    fn session_line(cwd: &str) -> String {
        format!(r#"{{"type":"user","cwd":"{cwd}","message":{{"content":"hello"}}}}"#)
    }

    #[test]
    fn group_sessions_classe_coordinateur_et_sous_agent() {
        let dir = tmp_projects_dir("group");
        let escaped = dir.join("-w-proj");
        write_with_mtime(&escaped.join("sidA.jsonl"), &session_line("/w/proj"), 0);
        write_with_mtime(
            &escaped.join("sidA/subagents/agent-1.jsonl"),
            "{}",
            0,
        );
        let sessions = group_sessions(&dir);
        assert_eq!(sessions.len(), 1);
        let (_key, raw) = sessions.iter().next().unwrap();
        assert!(raw.coordinator_transcript.is_some());
        assert_eq!(raw.subagents.len(), 1);
        assert_eq!(raw.subagents[0].agent_id, "1");
    }

    #[test]
    fn group_sessions_ignore_un_jsonl_de_subagents_sans_patron_agent() {
        let dir = tmp_projects_dir("journal");
        let escaped = dir.join("-w-proj");
        write_with_mtime(&escaped.join("sidA.jsonl"), &session_line("/w/proj"), 0);
        // Fichier d'orchestration hypothetique, pas du patron agent-<id>.jsonl : pas un agent.
        write_with_mtime(&escaped.join("sidA/subagents/journal.jsonl"), "{}", 0);
        let sessions = group_sessions(&dir);
        let (_key, raw) = sessions.iter().next().unwrap();
        assert!(raw.subagents.is_empty());
    }

    // ---------------------------- Etape 4 : arbre de delegation (D7) ----------------------------

    fn write_meta(path: &Path, agent_type: &str, parent: Option<&str>, depth: u32) {
        let parent_json = match parent {
            Some(p) => format!(r#","parentAgentId":"{p}""#),
            None => String::new(),
        };
        let json = format!(
            r#"{{"agentType":"{agent_type}","spawnDepth":{depth}{parent_json}}}"#
        );
        fs::write(path, json).unwrap();
    }

    #[test]
    fn arbre_parent_plus_deux_enfants() {
        let dir = tmp_projects_dir("arbre-2-enfants");
        let escaped = dir.join("-w-proj");
        write_with_mtime(&escaped.join("sid.jsonl"), &session_line("/w/proj"), 0);
        write_with_mtime(&escaped.join("sid/subagents/agent-1.jsonl"), "{}", 0);
        write_meta(&escaped.join("sid/subagents/agent-1.meta.json"), "gandalf", None, 1);
        write_with_mtime(&escaped.join("sid/subagents/agent-2.jsonl"), "{}", 0);
        write_meta(&escaped.join("sid/subagents/agent-2.meta.json"), "loki", None, 1);

        let snap = snapshot_at(&dir, now_for_tests(), 90);
        assert_eq!(snap.sessions.len(), 1);
        let coord = &snap.sessions[0].coordinator;
        assert_eq!(coord.sprite, coordinator_sprite());
        assert_eq!(coord.children.len(), 2);
        let letters: Vec<char> = coord.children.iter().map(|c| c.sprite.letter).collect();
        assert!(letters.contains(&'G')); // gandalf
        assert!(letters.contains(&'L')); // loki
        assert_eq!(snap.count, 3); // coordinateur + 2 enfants
    }

    #[test]
    fn arbre_petit_enfant_profondeur_2_est_imbrique() {
        let dir = tmp_projects_dir("arbre-imbrique");
        let escaped = dir.join("-w-proj");
        write_with_mtime(&escaped.join("sid.jsonl"), &session_line("/w/proj"), 0);
        write_with_mtime(&escaped.join("sid/subagents/agent-parent.jsonl"), "{}", 0);
        write_meta(&escaped.join("sid/subagents/agent-parent.meta.json"), "gandalf", None, 1);
        write_with_mtime(&escaped.join("sid/subagents/agent-child.jsonl"), "{}", 0);
        write_meta(
            &escaped.join("sid/subagents/agent-child.meta.json"),
            "nathalie",
            Some("parent"),
            2,
        );

        let snap = snapshot_at(&dir, now_for_tests(), 90);
        let coord = &snap.sessions[0].coordinator;
        assert_eq!(coord.children.len(), 1, "un seul enfant direct : gandalf");
        let gandalf = &coord.children[0];
        assert_eq!(gandalf.sprite.letter, 'G');
        assert_eq!(gandalf.children.len(), 1, "nathalie imbriquee sous gandalf");
        assert_eq!(gandalf.children[0].sprite.letter, 'N');
        assert_eq!(snap.count, 3); // coordinateur + gandalf + nathalie
    }

    #[test]
    fn arbre_parent_mort_enfant_vivant_remonte_au_plus_proche_ancetre_vivant() {
        let dir = tmp_projects_dir("parent-mort");
        let escaped = dir.join("-w-proj");
        write_with_mtime(&escaped.join("sid.jsonl"), &session_line("/w/proj"), 0);
        // Parent direct MORT (200 s, seuil 90 s).
        write_with_mtime(&escaped.join("sid/subagents/agent-parent.jsonl"), "{}", 200);
        write_meta(&escaped.join("sid/subagents/agent-parent.meta.json"), "gandalf", None, 1);
        // Enfant VIVANT dont le parent reel est le parent mort ci-dessus.
        write_with_mtime(&escaped.join("sid/subagents/agent-child.jsonl"), "{}", 0);
        write_meta(
            &escaped.join("sid/subagents/agent-child.meta.json"),
            "nathalie",
            Some("parent"),
            2,
        );

        let snap = snapshot_at(&dir, now_for_tests(), 90);
        let coord = &snap.sessions[0].coordinator;
        // Le parent mort n'est pas affiche ; l'enfant remonte directement sous le coordinateur.
        assert_eq!(coord.children.len(), 1);
        assert_eq!(coord.children[0].sprite.letter, 'N');
        assert!(coord.children[0].tooltip.contains("parent reel"));
        assert_eq!(snap.count, 2); // coordinateur + nathalie (le parent mort ne compte pas)
    }

    #[test]
    fn arbre_coordinateur_froid_avec_enfant_vivant_session_affichee() {
        let dir = tmp_projects_dir("coord-froid");
        let escaped = dir.join("-w-proj");
        // Coordinateur froid (200 s, seuil 90 s).
        write_with_mtime(&escaped.join("sid.jsonl"), &session_line("/w/proj"), 200);
        write_with_mtime(&escaped.join("sid/subagents/agent-1.jsonl"), "{}", 0);
        write_meta(&escaped.join("sid/subagents/agent-1.meta.json"), "helm", None, 1);

        let snap = snapshot_at(&dir, now_for_tests(), 90);
        assert_eq!(snap.sessions.len(), 1, "session affichee malgre coordinateur froid");
        assert_eq!(snap.sessions[0].coordinator.children.len(), 1);
    }

    #[test]
    fn arbre_session_sans_agent_vivant_est_absente() {
        let dir = tmp_projects_dir("tout-mort");
        let escaped = dir.join("-w-proj");
        write_with_mtime(&escaped.join("sid.jsonl"), &session_line("/w/proj"), 200);
        write_with_mtime(&escaped.join("sid/subagents/agent-1.jsonl"), "{}", 200);
        write_meta(&escaped.join("sid/subagents/agent-1.meta.json"), "helm", None, 1);

        let snap = snapshot_at(&dir, now_for_tests(), 90);
        assert!(snap.sessions.is_empty());
        assert_eq!(snap.count, 0);
    }

    #[test]
    fn arbre_meta_invalide_est_type_inconnu_gris_jamais_ignore() {
        let dir = tmp_projects_dir("meta-invalide");
        let escaped = dir.join("-w-proj");
        write_with_mtime(&escaped.join("sid.jsonl"), &session_line("/w/proj"), 0);
        write_with_mtime(&escaped.join("sid/subagents/agent-1.jsonl"), "{}", 0);
        fs::write(escaped.join("sid/subagents/agent-1.meta.json"), "pas du json").unwrap();

        let snap = snapshot_at(&dir, now_for_tests(), 90);
        let coord = &snap.sessions[0].coordinator;
        assert_eq!(coord.children.len(), 1, "agent affiche malgre sidecar invalide");
        assert_eq!(coord.children[0].sprite.bg, FALLBACK_BG);
    }

    #[test]
    fn arbre_borne_huit_enfants_avec_marqueur_overflow() {
        let dir = tmp_projects_dir("overflow-enfants");
        let escaped = dir.join("-w-proj");
        write_with_mtime(&escaped.join("sid.jsonl"), &session_line("/w/proj"), 0);
        for i in 0..10 {
            write_with_mtime(&escaped.join(format!("sid/subagents/agent-{i}.jsonl")), "{}", 0);
            write_meta(
                &escaped.join(format!("sid/subagents/agent-{i}.meta.json")),
                "helm",
                None,
                1,
            );
        }
        let snap = snapshot_at(&dir, now_for_tests(), 90);
        let coord = &snap.sessions[0].coordinator;
        // 8 enfants affiches + 1 marqueur de debordement.
        assert_eq!(coord.children.len(), 9);
        let marker = coord.children.last().unwrap();
        assert_eq!(marker.overflow, Some(2));
        // L'effectif total reste vrai (10 enfants + coordinateur), overflow inclus (D8).
        assert_eq!(snap.count, 11);
    }

    #[test]
    fn arbre_borne_six_sessions_avec_marqueur_overflow_sessions() {
        let dir = tmp_projects_dir("overflow-sessions");
        for i in 0..8 {
            let escaped = dir.join(format!("-w-proj{i}"));
            write_with_mtime(&escaped.join("sid.jsonl"), &session_line(&format!("/w/proj{i}")), 0);
        }
        let snap = snapshot_at(&dir, now_for_tests(), 90);
        assert_eq!(snap.sessions.len(), 6);
        assert_eq!(snap.overflow_sessions, Some(2));
    }

    // ---------------------------- Etape 5 : label de projet (D7) ----------------------------

    #[test]
    fn projet_cwd_normal_donne_le_nom_de_projet() {
        let dir = tmp_projects_dir("label-normal");
        let escaped = dir.join("-w-iaka-demo");
        write_with_mtime(&escaped.join("sid.jsonl"), &session_line("/w/iaka-demo"), 0);
        let snap = snapshot_at(&dir, now_for_tests(), 90);
        assert_eq!(snap.sessions[0].project, "iaka-demo");
    }

    #[test]
    fn projet_racine_de_portefeuille_tombe_hors_projet() {
        let dir = tmp_projects_dir("label-portefeuille");
        let escaped = dir.join("-w-work");
        write_with_mtime(&escaped.join("sid.jsonl"), &session_line("/w/work"), 0);
        let snap = snapshot_at(&dir, now_for_tests(), 90);
        assert_eq!(snap.sessions[0].project, "hors projet");
    }

    #[test]
    fn projet_fichier_vide_replie_sur_le_dossier_echappe() {
        let dir = tmp_projects_dir("label-vide");
        let escaped = dir.join("-w-mystere");
        write_with_mtime(&escaped.join("sid.jsonl"), "", 0);
        let snap = snapshot_at(&dir, now_for_tests(), 90);
        assert_eq!(snap.sessions[0].project, "-w-mystere");
    }

    #[test]
    fn projet_premiere_ligne_non_json_replie_sur_le_dossier_echappe() {
        let dir = tmp_projects_dir("label-non-json");
        let escaped = dir.join("-w-etrange");
        write_with_mtime(&escaped.join("sid.jsonl"), "pas du json\n", 0);
        let snap = snapshot_at(&dir, now_for_tests(), 90);
        assert_eq!(snap.sessions[0].project, "-w-etrange");
    }

    // ---------------------------- Emission-si-changement (D3) ----------------------------

    #[test]
    fn deux_instantanes_identiques_sont_egaux() {
        let dir = tmp_projects_dir("stabilite");
        let escaped = dir.join("-w-proj");
        write_with_mtime(&escaped.join("sid.jsonl"), &session_line("/w/proj"), 0);
        let now = now_for_tests();
        let a = snapshot_at(&dir, now, 90);
        let b = snapshot_at(&dir, now, 90);
        assert_eq!(a, b, "meme entree, meme instantane -> PartialEq permet de ne pas re-emettre");
    }

    /// Horloge murale figee pour les tests d'arbre (les fichiers sont ecrits "maintenant" ou
    /// "vieillis" via `write_with_mtime` ; on compare toujours a `SystemTime::now()` au moment du
    /// test, comme le ferait le watcher en production).
    fn now_for_tests() -> i64 {
        crate::memory::now_secs()
    }
}
