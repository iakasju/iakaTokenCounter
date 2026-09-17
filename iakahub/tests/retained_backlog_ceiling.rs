//! Test d'integration de caracterisation du plafond de rattrapage retained
//! (garde-plafond-retained-broker.md, C4/C5) : contre le **vrai** broker `iakahub::broker`, sur
//! **port libre** (jamais 1883 — le broker de l'app installee y ecoute), on fige les deux regimes
//! mesures au cadrage :
//!
//! - **sous le seuil** (C4) : 227 topics retained publies, dont 39 sous les deux filtres du tray
//!   (`iakatc_core::publish::contract::consumer_filters`) ; un abonne qui pose ces filtres **apres
//!   coup** recoit **39/39** — la preuve que le tray n'est pas expose aujourd'hui ;
//! - **hors seuil** (C5) : 150 topics retained sous un **seul** filtre ; l'abonne recoit
//!   **exactement 100** (`RETAINED_FANOUT_CEILING`), et ce compte **n'evolue plus** apres une
//!   attente bornee supplementaire — la troncature est **definitive**, pas un debit bride.
//!
//! Reprend le motif `free_port()` de `broker_roundtrip.rs`. Les messages publies reutilisent les
//! constructeurs reels du contrat (`iakatc-core`, dev-dependency) : la fixture "sous le seuil" est
//! donc representative du vrai tick (5 reservoirs/4 comptes, 44 projets, 1 couple provider/agent),
//! pas une approximation ad hoc.

use std::collections::BTreeMap;
use std::net::TcpListener;
use std::time::{Duration, Instant};

use rumqttc::{Client, Event, MqttOptions, Packet, QoS};

use iakatc_core::measure::{Agent, Provider, Tokens};
use iakatc_core::publish::contract::{
    conso_project_agent, conso_provider_agent, consumer_filters, limits, meta, pairs_of, quota,
    RETAINED_FANOUT_CEILING,
};
use iakatc_core::publish::Message;
use iakatc_core::quota::config::Config;
use iakatc_core::quota::merge::{Confidence, Reservoir, Source, Window};

const T: i64 = 1751894400;

/// Reserve un port libre puis le relache : le broker le reprendra juste apres.
fn free_port() -> u16 {
    let l = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemere");
    l.local_addr().unwrap().port()
}

fn reservoir(account: &str, window: Window) -> Reservoir {
    Reservoir {
        provider: "claude".into(),
        account: account.into(),
        window,
        used_pct: Some(10.0),
        remaining_pct: Some(90.0),
        used_tokens: Some(1000),
        resets_at: Some(T),
        captured_at: Some(T),
        confidence: Confidence::Official,
        source: Some(Source::Statusline),
    }
}

/// Reconstruit le tick representatif fige par le lot (meme fixture, testee unitairement dans
/// `iakatc-core/src/publish/contract.rs`) : 227 topics retained au total dont **39** sous les
/// filtres du tray (35 quota + 4 meta).
fn representative_tick(root: &str) -> Vec<Message> {
    let reservoirs = vec![
        reservoir("acct0", Window::FiveHour),
        reservoir("acct0", Window::SevenDay),
        reservoir("acct1", Window::FiveHour),
        reservoir("acct2", Window::FiveHour),
        reservoir("acct3", Window::FiveHour),
    ];
    let mut messages = quota(root, &reservoirs, T);
    messages.extend(limits(root, &Config::default(), &pairs_of(&reservoirs), T));

    let mut by_project = BTreeMap::new();
    for i in 0..44 {
        by_project.insert(
            (format!("project{i}"), Agent::Coordinator),
            Tokens {
                input: 1,
                output: 1,
                cache: 1,
            },
        );
    }
    messages.extend(conso_project_agent(root, &by_project, T));

    let mut by_provider = BTreeMap::new();
    by_provider.insert(
        (Provider::Claude, Agent::Coordinator),
        Tokens {
            input: 1,
            output: 1,
            cache: 1,
        },
    );
    messages.extend(conso_provider_agent(root, &by_provider, T));

    messages.extend(meta(root, "up", T, true, "0.1.0", T));

    assert_eq!(messages.len(), 227, "fixture attendue a 227 topics au total");
    messages
}

/// 150 topics concrets, tous sous le **meme** filtre de consommateur
/// (`{root}/all/ia/+/+/quota/#`), pour le cas hors seuil (un seul filtre : cf. cause racine § 4 de
/// l'instruction — deux filtres se partageraient la meme fenetre de 100 et rendraient le
/// "exactement 100" flou).
fn synthetic_quota_topics(root: &str, n: usize) -> Vec<Message> {
    (0..n)
        .map(|i| Message {
            topic: format!("{root}/all/ia/claude/acct{i}/quota/5h/used_pct/current"),
            payload: format!(r#"{{"v":10,"t":{T}}}"#),
        })
        .collect()
}

/// Publie chaque message en retained QoS 1, puis attend l'acquittement (PubAck) de tous avant de
/// se deconnecter. Capacite du channel >= au nombre de messages : aucun `publish()` ne bloque
/// avant que l'event-loop soit pompe.
fn publish_all_retained(port: u16, messages: &[Message]) {
    let mut opts = MqttOptions::new("iakahub-test-pub", "127.0.0.1", port);
    opts.set_keep_alive(Duration::from_secs(5));
    let (client, mut connection) = Client::new(opts, messages.len() + 16);

    for m in messages {
        client
            .publish(
                m.topic.clone(),
                QoS::AtLeastOnce,
                true,
                m.payload.clone().into_bytes(),
            )
            .expect("publish retained");
    }

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut acked = 0usize;
    for notif in connection.iter() {
        if let Ok(Event::Incoming(Packet::PubAck(_))) = notif {
            acked += 1;
            if acked >= messages.len() {
                break;
            }
        }
        if Instant::now() > deadline {
            break;
        }
    }
    assert_eq!(
        acked,
        messages.len(),
        "tous les retained doivent etre acquittes avant de continuer"
    );
    client.disconnect().ok();
}

/// C4 — sous le seuil : 227 topics retained publies (dont 39 sous les filtres du tray), abonne
/// pose **apres coup** avec les deux filtres du tray ⇒ 39/39 recus.
#[test]
fn c4_sous_le_seuil_labonne_recoit_tout_ce_qui_matche_les_filtres_du_tray() {
    let port = free_port();
    let addr = iakahub::broker::start(port).expect("le broker local doit demarrer");
    assert_eq!(addr.port(), port);

    let root = "iakatokencounter";
    let messages = representative_tick(root);
    publish_all_retained(port, &messages);

    let filters = consumer_filters(root);
    let mut opts = MqttOptions::new("iakahub-test-sub-sous-seuil", "127.0.0.1", port);
    opts.set_keep_alive(Duration::from_secs(5));
    let (sub_client, mut sub_conn) = Client::new(opts, 64);
    for f in &filters {
        sub_client.subscribe(f, QoS::AtLeastOnce).expect("subscribe");
    }

    let mut received = 0usize;
    let deadline = Instant::now() + Duration::from_secs(10);
    for notif in sub_conn.iter() {
        if let Ok(Event::Incoming(Packet::Publish(_))) = notif {
            received += 1;
            if received >= 39 {
                break;
            }
        }
        if Instant::now() > deadline {
            break;
        }
    }
    sub_client.disconnect().ok();

    assert_eq!(
        received, 39,
        "39/39 attendus : le tray n'est pas expose sous le seuil (35 quota + 4 meta)"
    );
}

/// C5 — hors seuil : 150 topics retained sous **un seul** filtre, abonne pose apres coup ⇒
/// exactement 100 recus (`RETAINED_FANOUT_CEILING`), et le compte n'evolue plus apres une seconde
/// fenetre d'observation (>= 3 s) sans aucune publication concurrente — preuve que la troncature
/// est definitive, pas un debit bride.
///
/// Risque connu (§ Risques de l'instruction) : ce test affirme une absence, ce qui se prouve mal.
/// S'il devient flaky a l'execution, le marquer `#[ignore]` avec une justification plutot que
/// d'affaiblir l'assertion `== RETAINED_FANOUT_CEILING`.
#[test]
fn c5_hors_seuil_la_troncature_est_definitive_pas_un_debit_bride() {
    let port = free_port();
    let addr = iakahub::broker::start(port).expect("le broker local doit demarrer");
    assert_eq!(addr.port(), port);

    let root = "iakatokencounter";
    let messages = synthetic_quota_topics(root, 150);
    publish_all_retained(port, &messages);

    let filters = consumer_filters(root);
    let quota_filter = &filters[0];

    let mut opts = MqttOptions::new("iakahub-test-sub-hors-seuil", "127.0.0.1", port);
    opts.set_keep_alive(Duration::from_secs(5));
    let (sub_client, mut sub_conn) = Client::new(opts, 64);
    sub_client
        .subscribe(quota_filter, QoS::AtLeastOnce)
        .expect("subscribe");

    // Premiere fenetre d'observation, bornee mais genereuse : laisse le temps a la troncature de
    // se stabiliser (mesure au cadrage : 100/300 en 10 s).
    let mut received = 0usize;
    let deadline = Instant::now() + Duration::from_secs(10);
    for notif in sub_conn.iter() {
        if let Ok(Event::Incoming(Packet::Publish(_))) = notif {
            received += 1;
        }
        if Instant::now() > deadline {
            break;
        }
    }
    assert_eq!(
        received, RETAINED_FANOUT_CEILING,
        "le plafond dur de rumqttd doit tronquer exactement a {RETAINED_FANOUT_CEILING}"
    );

    // Seconde fenetre d'observation (>= 3 s), sans aucune publication concurrente : le compte ne
    // doit plus evoluer.
    let deadline2 = Instant::now() + Duration::from_secs(3);
    for notif in sub_conn.iter() {
        if let Ok(Event::Incoming(Packet::Publish(_))) = notif {
            received += 1;
        }
        if Instant::now() > deadline2 {
            break;
        }
    }
    sub_client.disconnect().ok();

    assert_eq!(
        received, RETAINED_FANOUT_CEILING,
        "aucun message supplementaire ne doit arriver apres la troncature (definitive, pas bridee)"
    );
}
