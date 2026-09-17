//! history — commande `get_history(provider)` de la vue analytics (D2/D3).
//!
//! **Source = relecture disque via `iatc-core`** (option a, D2) : aucune persistance nouvelle. A
//! l'ouverture de la vue (et sur « Rafraichir »), le backend re-scanne les logs locaux du provider
//! et renvoie l'activite *all-time* (tokens/jour/projet) + le cout par projet (coord/sub). Les
//! fonctions de scan sont **read-only** et vivent dans `iatc-core` (le daemon reste figE).
//!
//! **Granularite (D4)** : l'historique est ventile par **projet** et **coord/sub**, a l'echelle du
//! **provider** (les JSONL/rollouts ne portent pas l'ID de compte). Le quota courant par compte est
//! gere ailleurs (etat MQTT retained, `get_reservoirs`).
//!
//! Defensif : un dossier de logs absent -> series **vides**, jamais d'erreur.

use iakatc_core::measure::claude::{
    claude_projects_dir, scan_projects_activity, scan_projects_dir, ProjectActivity, ProjectEconomy,
};
use iakatc_core::measure::codex::{codex_sessions_dir, scan_codex_activity, scan_codex_measurements};
use serde::Serialize;
use std::path::Path;

/// Nombre max de projets remontes par serie (les viz bornent l'affichage ; scroll au-dela).
const HISTORY_TOP: usize = 20;

/// Provider d'historique demande par la vue analytics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryProvider {
    Claude,
    Codex,
}

impl HistoryProvider {
    /// Parse le code du provider (`claude` / `codex`). `None` = provider inconnu.
    fn from_code(s: &str) -> Option<HistoryProvider> {
        match s {
            "claude" => Some(HistoryProvider::Claude),
            "codex" => Some(HistoryProvider::Codex),
            _ => None,
        }
    }
}

/// Charge utile renvoyee a la webview : deux series homogenes entre providers.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPayload {
    /// Tokens/jour/projet (timeline). Jours tries croissants, projets par total desc.
    pub activity: Vec<ProjectActivity>,
    /// Cout par projet + split coordinateur/sous-agent (treemap + split).
    pub economy: Vec<ProjectEconomy>,
}

impl HistoryPayload {
    fn empty() -> Self {
        HistoryPayload {
            activity: Vec::new(),
            economy: Vec::new(),
        }
    }
}

/// Convertit les mesures Codex (agent = coordinator seul) en `ProjectEconomy` : tout l'output est
/// du coordinateur (`sub = 0`, Codex n'a pas de sidechain). Trie par cout total decroissant.
fn codex_economy(sessions_root: &Path, top: usize) -> Vec<ProjectEconomy> {
    let mut out: Vec<ProjectEconomy> = scan_codex_measurements(sessions_root)
        .into_iter()
        .map(|m| ProjectEconomy {
            project: m.project,
            input: m.tokens.input,
            output: m.tokens.output,
            coord: m.tokens.output, // Codex = coordinateur uniquement.
            sub: 0,
            // Infobulle cwd (D4) hors perimetre Codex dans ce lot : ni le doublonnage ni les
            // sous-agents ne le concernent (`specs/instructions/feature-verite-des-chiffres.md`
            // § Hors scope). Chaine vide = pas de cwd d'exemple, traitement defensif deja prevu
            // cote webview.
            example_cwd: String::new(),
        })
        .collect();
    out.sort_by_key(|p| std::cmp::Reverse(p.input + p.output));
    out.truncate(top);
    out
}

/// Prepare les series d'historique pour un provider. **Pur et testable** (prend les dossiers de
/// logs en parametre). Dossier absent (`None`) ou illisible -> series vides.
pub fn build_history(
    provider: HistoryProvider,
    claude_dir: Option<&Path>,
    codex_dir: Option<&Path>,
) -> HistoryPayload {
    match provider {
        HistoryProvider::Claude => match claude_dir {
            Some(dir) => HistoryPayload {
                activity: scan_projects_activity(dir, HISTORY_TOP),
                economy: scan_projects_dir(dir, HISTORY_TOP),
            },
            None => HistoryPayload::empty(),
        },
        HistoryProvider::Codex => match codex_dir {
            Some(dir) => HistoryPayload {
                activity: scan_codex_activity(dir, HISTORY_TOP),
                economy: codex_economy(dir, HISTORY_TOP),
            },
            None => HistoryPayload::empty(),
        },
    }
}

/// Commande Tauri : historique *all-time* du provider, relu du disque via `iatc-core`.
/// Provider inconnu -> `Err`. Dossier de logs absent -> `Ok` avec series vides (pas d'erreur).
#[tauri::command]
pub fn get_history(provider: String) -> Result<HistoryPayload, String> {
    let provider = HistoryProvider::from_code(&provider)
        .ok_or_else(|| format!("provider inconnu : {provider}"))?;
    let claude_dir = claude_projects_dir();
    let codex_dir = codex_sessions_dir();
    Ok(build_history(
        provider,
        claude_dir.as_deref(),
        codex_dir.as_deref(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Racine des fixtures Claude du repo (multi-projets, coord + sidechain, ligne corrompue).
    fn mock_claude() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../specs/mock/claude_projects")
    }

    #[test]
    fn provider_from_code_connu_et_inconnu() {
        assert_eq!(
            HistoryProvider::from_code("claude"),
            Some(HistoryProvider::Claude)
        );
        assert_eq!(
            HistoryProvider::from_code("codex"),
            Some(HistoryProvider::Codex)
        );
        assert_eq!(HistoryProvider::from_code("gemini"), None);
    }

    #[test]
    fn build_history_claude_sur_fixtures() {
        let dir = mock_claude();
        let h = build_history(HistoryProvider::Claude, Some(&dir), None);
        // Trois seaux : alpha, beta, et "hors projet" (fixture -w-work, racine de portefeuille,
        // D4). Series non vides.
        assert_eq!(h.activity.len(), 3);
        assert_eq!(h.economy.len(), 3);
        let projects: Vec<&str> = h.economy.iter().map(|e| e.project.as_str()).collect();
        assert!(projects.contains(&"alpha"));
        assert!(projects.contains(&"beta"));
        assert!(projects.contains(&"hors projet"));
        // alpha porte du coordinateur ET du sous-agent (sidechain, y compris ceux de
        // subagents/agent-1.jsonl, D1) -> split honnete.
        let alpha = h.economy.iter().find(|e| e.project == "alpha").unwrap();
        assert!(alpha.coord > 0, "coord alpha attendu > 0");
        assert!(alpha.sub > 0, "sub alpha (sidechain) attendu > 0");
        // Toute l'activite est datee (jours tries) et non vide.
        assert!(h.activity.iter().all(|p| !p.days.is_empty()));
    }

    #[test]
    fn build_history_dossier_absent_series_vides() {
        // Provider Claude mais dossier None -> vide, pas d'erreur.
        let h = build_history(HistoryProvider::Claude, None, None);
        assert!(h.activity.is_empty() && h.economy.is_empty());
        // Dossier fourni mais inexistant -> vide aussi (defensif iatc-core).
        let bogus = PathBuf::from("/dossier/inexistant/xyz");
        let h2 = build_history(HistoryProvider::Codex, None, Some(&bogus));
        assert!(h2.activity.is_empty() && h2.economy.is_empty());
    }

    #[test]
    fn build_history_codex_utilise_le_dossier_codex_pas_claude() {
        // Provider Codex ne lit QUE le dossier codex : un dossier claude fourni ne le pollue pas.
        let claude = mock_claude();
        let h = build_history(HistoryProvider::Codex, Some(&claude), None);
        assert!(h.activity.is_empty() && h.economy.is_empty());
    }

    #[test]
    fn payload_serialise_en_camel_case() {
        let dir = mock_claude();
        let h = build_history(HistoryProvider::Claude, Some(&dir), None);
        let json = serde_json::to_string(&h).unwrap();
        assert!(json.contains("\"activity\""), "{json}");
        assert!(json.contains("\"economy\""), "{json}");
        // ProjectEconomy expose coord/sub (split).
        assert!(json.contains("\"coord\""), "{json}");
        assert!(json.contains("\"sub\""), "{json}");
    }
}
