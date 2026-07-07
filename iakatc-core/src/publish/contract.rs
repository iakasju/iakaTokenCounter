//! publish::contract — construction des topics-codes et payloads `{v,t}` du contrat (§ 2/§ 3).
//!
//! Chaque feuille de l'arbre de topics est **un code scalaire** ; chaque message porte
//! `{"v":<scalaire>,"t":<epoch_s>}`. Une valeur inconnue est publiee `{"v":null,...}` (jamais
//! d'absence deguisee). Ce module est **teste contre les exemples concrets** du contrat.

use crate::aggregate;
use crate::measure::{Agent, Provider, Tokens};
use crate::publish::Message;
use crate::quota::config::Config;
use crate::quota::merge::Reservoir;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// Suffixe d'etat par defaut : la valeur vivante du tick courant.
const CURRENT: &str = "current";

/// Payload scalaire `{v,t}` — l'ordre des champs (`v` puis `t`) est garanti par serde (struct).
#[derive(Serialize)]
struct Payload {
    v: Value,
    t: i64,
}

/// Serialise un couple `(v, t)` en `{"v":...,"t":...}`.
fn payload(v: Value, t: i64) -> String {
    serde_json::to_string(&Payload { v, t }).expect("payload scalaire toujours serialisable")
}

/// Les 4 codes de conso d'un bloc de tokens, dans l'ordre du contrat (§ 3.1).
fn conso_codes(tokens: &Tokens) -> [(&'static str, u64); 4] {
    [
        ("input_tokens", tokens.input),
        ("output_tokens", tokens.output),
        ("cache_tokens", tokens.cache),
        ("used_tokens", tokens.used()),
    ]
}

/// Axe 1 (projet x agent) : `{root}/all/projets/agents/{project}/{agent}/conso/{code}/current`.
pub fn conso_project_agent(
    root: &str,
    by_project_agent: &BTreeMap<(String, Agent), Tokens>,
    t: i64,
) -> Vec<Message> {
    let mut out = Vec::new();
    for ((project, agent), tokens) in by_project_agent {
        for (code, value) in conso_codes(tokens) {
            out.push(Message {
                topic: format!(
                    "{root}/all/projets/agents/{project}/{}/conso/{code}/{CURRENT}",
                    agent.code()
                ),
                payload: payload(Value::from(value), t),
            });
        }
    }
    out
}

/// Axe 2 (ia x agent) : `{root}/all/ia/agents/{provider}/{agent}/conso/{code}/current`.
pub fn conso_provider_agent(
    root: &str,
    by_provider_agent: &BTreeMap<(Provider, Agent), Tokens>,
    t: i64,
) -> Vec<Message> {
    let mut out = Vec::new();
    for ((provider, agent), tokens) in by_provider_agent {
        for (code, value) in conso_codes(tokens) {
            out.push(Message {
                topic: format!(
                    "{root}/all/ia/agents/{}/{}/conso/{code}/{CURRENT}",
                    provider.code(),
                    agent.code()
                ),
                payload: payload(Value::from(value), t),
            });
        }
    }
    out
}

/// Quota : `{root}/all/ia/{provider}/{account}/quota/{window}/{code}/current`. Le `Reservoir` est
/// **decompose en codes scalaires atomiques** (§ 3.3 : `confidence`/`source` sont des codes voisins,
/// pas noyes dans un objet).
pub fn quota(root: &str, reservoirs: &[Reservoir], t: i64) -> Vec<Message> {
    let mut out = Vec::new();
    for r in reservoirs {
        let prefix = format!(
            "{root}/all/ia/{}/{}/quota/{}",
            r.provider,
            r.account,
            r.window.code()
        );
        let num = |o: Option<f64>| o.map(Value::from).unwrap_or(Value::Null);
        let numu = |o: Option<u64>| o.map(Value::from).unwrap_or(Value::Null);
        let numi = |o: Option<i64>| o.map(Value::from).unwrap_or(Value::Null);
        let push = |out: &mut Vec<Message>, code: &str, v: Value| {
            out.push(Message {
                topic: format!("{prefix}/{code}/{CURRENT}"),
                payload: payload(v, t),
            });
        };
        push(&mut out, "used_pct", num(r.used_pct));
        push(&mut out, "remaining_pct", num(r.remaining_pct));
        push(&mut out, "used_tokens", numu(r.used_tokens));
        push(&mut out, "resets_at", numi(r.resets_at));
        push(&mut out, "captured_at", numi(r.captured_at));
        push(&mut out, "confidence", Value::from(r.confidence.code()));
        push(
            &mut out,
            "source",
            r.source.map(|s| Value::from(s.code())).unwrap_or(Value::Null),
        );
    }
    out
}

/// Limits : `{root}/all/ia/{provider}/{account}/limits/{ceiling_5h_tokens|ceiling_7d_tokens}/current`.
/// Les plafonds viennent de la config (null par defaut). `pairs` = couples `(provider, account)` a
/// publier (typiquement ceux des reservoirs).
pub fn limits(root: &str, config: &Config, pairs: &[(String, String)], t: i64) -> Vec<Message> {
    let mut out = Vec::new();
    for (provider, account) in pairs {
        let ceiling = config.ceiling(provider, account);
        let prefix = format!("{root}/all/ia/{provider}/{account}/limits");
        let numu = |o: Option<u64>| o.map(Value::from).unwrap_or(Value::Null);
        out.push(Message {
            topic: format!("{prefix}/ceiling_5h_tokens/{CURRENT}"),
            payload: payload(numu(ceiling.five_hour_tokens), t),
        });
        out.push(Message {
            topic: format!("{prefix}/ceiling_7d_tokens/{CURRENT}"),
            payload: payload(numu(ceiling.seven_day_tokens), t),
        });
    }
    out
}

/// Etat de sante du daemon : `{root}/meta/daemon/{code}/current`.
pub fn meta(
    root: &str,
    state: &str,
    last_tick_at: i64,
    broker_connected: bool,
    version: &str,
    t: i64,
) -> Vec<Message> {
    vec![
        Message {
            topic: format!("{root}/meta/daemon/state/{CURRENT}"),
            payload: payload(Value::from(state), t),
        },
        Message {
            topic: format!("{root}/meta/daemon/last_tick_at/{CURRENT}"),
            payload: payload(Value::from(last_tick_at), t),
        },
        Message {
            topic: format!("{root}/meta/daemon/broker_connected/{CURRENT}"),
            payload: payload(Value::from(broker_connected), t),
        },
        Message {
            topic: format!("{root}/meta/daemon/version/{CURRENT}"),
            payload: payload(Value::from(version), t),
        },
    ]
}

/// Couples `(provider, account)` distincts presents dans les reservoirs (pour les `limits`).
pub fn pairs_of(reservoirs: &[Reservoir]) -> Vec<(String, String)> {
    let mut seen = std::collections::BTreeSet::new();
    for r in reservoirs {
        seen.insert((r.provider.clone(), r.account.clone()));
    }
    seen.into_iter().collect()
}

/// Convenance : produit TOUS les messages d'un tick. `root` = racine de topic, `t` = epoch s.
#[allow(clippy::too_many_arguments)]
pub fn tick_messages(
    root: &str,
    measurements: &[crate::measure::Measurement],
    reservoirs: &[Reservoir],
    config: &Config,
    state: &str,
    last_tick_at: i64,
    broker_connected: bool,
    version: &str,
    t: i64,
) -> Vec<Message> {
    let mut out = Vec::new();
    out.extend(conso_project_agent(
        root,
        &aggregate::by_project_agent(measurements),
        t,
    ));
    out.extend(conso_provider_agent(
        root,
        &aggregate::by_provider_agent(measurements),
        t,
    ));
    out.extend(quota(root, reservoirs, t));
    out.extend(limits(root, config, &pairs_of(reservoirs), t));
    out.extend(meta(
        root,
        state,
        last_tick_at,
        broker_connected,
        version,
        t,
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::merge::{Confidence, Source, Window};

    const T: i64 = 1751894400;

    fn topic<'a>(msgs: &'a [Message], topic: &str) -> &'a Message {
        msgs.iter()
            .find(|m| m.topic == topic)
            .unwrap_or_else(|| panic!("topic absent : {topic}\nemis: {:#?}", msgs.iter().map(|m| &m.topic).collect::<Vec<_>>()))
    }

    #[test]
    fn conso_projet_agent_topic_et_payload_exacts() {
        let mut map = BTreeMap::new();
        map.insert(
            ("iakaTokenCounter".to_string(), Agent::Coordinator),
            Tokens {
                input: 90000,
                output: 0,
                cache: 0,
            },
        );
        let msgs = conso_project_agent("iakatokencounter", &map, T);
        // Exemple exact du contrat (§ 2, tableau).
        let m = topic(
            &msgs,
            "iakatokencounter/all/projets/agents/iakaTokenCounter/coordinator/conso/input_tokens/current",
        );
        assert_eq!(m.payload, r#"{"v":90000,"t":1751894400}"#);
    }

    #[test]
    fn conso_ia_agent_codex_used_tokens_exact() {
        let mut map = BTreeMap::new();
        map.insert(
            (Provider::Codex, Agent::Coordinator),
            Tokens {
                input: 45000,
                output: 0,
                cache: 0,
            },
        );
        let msgs = conso_provider_agent("iakatokencounter", &map, T);
        let m = topic(
            &msgs,
            "iakatokencounter/all/ia/agents/codex/coordinator/conso/used_tokens/current",
        );
        assert_eq!(m.payload, r#"{"v":45000,"t":1751894400}"#);
    }

    #[test]
    fn quota_decompose_en_codes_scalaires_exacts() {
        let r = Reservoir {
            provider: "claude".into(),
            account: "max".into(),
            window: Window::FiveHour,
            used_pct: Some(12.5),
            remaining_pct: Some(87.5),
            used_tokens: Some(123),
            resets_at: Some(1751864400),
            captured_at: Some(1751846100),
            confidence: Confidence::Official,
            source: Some(Source::Statusline),
        };
        let msgs = quota("iakatokencounter", &[r], T);
        // Exemples exacts du contrat (§ 2, tableau).
        assert_eq!(
            topic(&msgs, "iakatokencounter/all/ia/claude/max/quota/5h/remaining_pct/current").payload,
            r#"{"v":87.5,"t":1751894400}"#
        );
        assert_eq!(
            topic(&msgs, "iakatokencounter/all/ia/claude/max/quota/5h/used_pct/current").payload,
            r#"{"v":12.5,"t":1751894400}"#
        );
        assert_eq!(
            topic(&msgs, "iakatokencounter/all/ia/claude/max/quota/5h/resets_at/current").payload,
            r#"{"v":1751864400,"t":1751894400}"#
        );
        assert_eq!(
            topic(&msgs, "iakatokencounter/all/ia/claude/max/quota/5h/confidence/current").payload,
            r#"{"v":"official","t":1751894400}"#
        );
    }

    #[test]
    fn quota_none_publie_v_null() {
        let r = Reservoir {
            provider: "claude".into(),
            account: "default".into(),
            window: Window::SevenDay,
            used_pct: None,
            remaining_pct: None,
            used_tokens: Some(4242),
            resets_at: None,
            captured_at: None,
            confidence: Confidence::None,
            source: None,
        };
        let msgs = quota("iakatokencounter", &[r], T);
        assert_eq!(
            topic(&msgs, "iakatokencounter/all/ia/claude/default/quota/7d/used_pct/current").payload,
            r#"{"v":null,"t":1751894400}"#
        );
        assert_eq!(
            topic(&msgs, "iakatokencounter/all/ia/claude/default/quota/7d/used_tokens/current").payload,
            r#"{"v":4242,"t":1751894400}"#
        );
        assert_eq!(
            topic(&msgs, "iakatokencounter/all/ia/claude/default/quota/7d/source/current").payload,
            r#"{"v":null,"t":1751894400}"#
        );
    }

    #[test]
    fn limits_ceiling_null_par_defaut_exact() {
        let pairs = vec![("claude".to_string(), "max".to_string())];
        let msgs = limits("iakatokencounter", &Config::default(), &pairs, T);
        assert_eq!(
            topic(&msgs, "iakatokencounter/all/ia/claude/max/limits/ceiling_7d_tokens/current").payload,
            r#"{"v":null,"t":1751894400}"#
        );
    }

    #[test]
    fn meta_state_up_exact() {
        let msgs = meta("iakatokencounter", "up", T, true, "0.1.0", T);
        assert_eq!(
            topic(&msgs, "iakatokencounter/meta/daemon/state/current").payload,
            r#"{"v":"up","t":1751894400}"#
        );
        assert_eq!(
            topic(&msgs, "iakatokencounter/meta/daemon/broker_connected/current").payload,
            r#"{"v":true,"t":1751894400}"#
        );
    }

    #[test]
    fn environnement_vide_ne_publie_que_meta() {
        // Aucune mesure, aucun reservoir -> seuls les codes meta/daemon sont emis.
        let msgs = tick_messages(
            "iakatokencounter",
            &[],
            &[],
            &Config::default(),
            "up",
            T,
            false,
            "0.1.0",
            T,
        );
        assert!(msgs.iter().all(|m| m.topic.starts_with("iakatokencounter/meta/daemon/")));
        assert_eq!(msgs.len(), 4); // state, last_tick_at, broker_connected, version
    }
}
