//! aggregate — re-sommation des mesures selon les DEUX axes du contrat (§ 0, D6).
//!
//! - Axe 1 (projet x agent) : `all/projets/agents/{project}/{agent}/conso/...` — somme des tokens
//!   par `(project, agent)`, **tous providers confondus** (le topic n'a pas de segment provider).
//! - Axe 2 (ia x agent) : `all/ia/agents/{provider}/{agent}/conso/...` — memes tokens **re-sommes**
//!   par `(provider, agent)`, tous projets confondus.
//!
//! On utilise des `BTreeMap` pour un ordre de sortie **deterministe** (tests + lisibilite).

use crate::measure::{Agent, Measurement, Provider, Tokens};
use std::collections::BTreeMap;

/// Agrege les mesures par `(project, agent)` (axe 1 du contrat). Cle triee : projet puis agent.
pub fn by_project_agent(measurements: &[Measurement]) -> BTreeMap<(String, Agent), Tokens> {
    let mut acc: BTreeMap<(String, Agent), Tokens> = BTreeMap::new();
    for m in measurements {
        acc.entry((m.project.clone(), m.agent))
            .or_default()
            .add(&m.tokens);
    }
    acc
}

/// Agrege les mesures par `(provider, agent)` (axe 2 du contrat). Cle triee : provider puis agent.
pub fn by_provider_agent(measurements: &[Measurement]) -> BTreeMap<(Provider, Agent), Tokens> {
    let mut acc: BTreeMap<(Provider, Agent), Tokens> = BTreeMap::new();
    for m in measurements {
        acc.entry((m.provider, m.agent))
            .or_default()
            .add(&m.tokens);
    }
    acc
}

/// Somme des `used_tokens` mesures pour un provider donne (tous projets/agents). Sert de
/// diagnostic `used_tokens` cote quota (le JSONL ne porte pas d'`account` -> total du provider).
pub fn used_tokens_by_provider(measurements: &[Measurement], provider: Provider) -> u64 {
    measurements
        .iter()
        .filter(|m| m.provider == provider)
        .map(|m| m.tokens.used())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(project: &str, provider: Provider, agent: Agent, input: u64, output: u64) -> Measurement {
        Measurement {
            project: project.into(),
            provider,
            agent,
            tokens: Tokens {
                input,
                output,
                cache: 0,
            },
        }
    }

    #[test]
    fn axe_projet_agent_somme_tous_providers() {
        let ms = vec![
            m("P", Provider::Claude, Agent::Coordinator, 10, 5),
            m("P", Provider::Codex, Agent::Coordinator, 3, 2),
            m("P", Provider::Claude, Agent::Subagent, 8, 4),
        ];
        let a = by_project_agent(&ms);
        // Coordinator sur P = Claude(15) + Codex(5) = 20, tous providers confondus.
        assert_eq!(a[&("P".into(), Agent::Coordinator)].used(), 20);
        assert_eq!(a[&("P".into(), Agent::Subagent)].used(), 12);
    }

    #[test]
    fn axe_ia_agent_resomme_par_provider() {
        let ms = vec![
            m("P", Provider::Claude, Agent::Coordinator, 10, 5),
            m("Q", Provider::Claude, Agent::Coordinator, 1, 1),
            m("P", Provider::Codex, Agent::Coordinator, 3, 2),
        ];
        let a = by_provider_agent(&ms);
        // Claude coordinator (tous projets) = 15 + 2 = 17.
        assert_eq!(a[&(Provider::Claude, Agent::Coordinator)].used(), 17);
        assert_eq!(a[&(Provider::Codex, Agent::Coordinator)].used(), 5);
    }

    #[test]
    fn used_tokens_par_provider() {
        let ms = vec![
            m("P", Provider::Claude, Agent::Coordinator, 10, 5),
            m("P", Provider::Claude, Agent::Subagent, 8, 2),
            m("P", Provider::Codex, Agent::Coordinator, 3, 2),
        ];
        assert_eq!(used_tokens_by_provider(&ms, Provider::Claude), 25);
        assert_eq!(used_tokens_by_provider(&ms, Provider::Codex), 5);
    }
}
