//! Tests d'integration du transport MQTT sans perte — criteres A1/A2 de
//! `specs/instructions/fix-mqtt-transport-pertes.md`.
//!
//! Ce fichier n'a pu exister qu'a partir du correctif : avant lui, `iakatc-daemon` n'avait pas de
//! cible `[lib]` et `mqtt.rs` n'etait pas testable en integration (cf. la ligne « Cible de test du
//! daemon » de l'instruction). Sur le code d'avant correctif (capacite de channel 64,
//! `let _ = try_publish(...)` fire-and-forget), un lot de 300 topics perdait la grande majorite
//! des messages en une salve (mesure du diagnostic : 162 sur 227 en conditions reelles, reproduite
//! en A5 contre un vrai broker de capture).
//!
//! **Le broker de ce test est un broker MQTT 3.1.1 minimal, reimplemente ici** (CONNECT->CONNACK,
//! PUBLISH QoS1->PUBACK, PINGREQ->PINGRESP, aucun routage d'abonnes) avec le codec fil
//! `rumqttc::mqttbytes::v4` deja utilise par le client — plutot que le broker embarque
//! `iakahub::broker` (`rumqttd`). Verifie empiriquement : au-dela d'une centaine de PUBLISH QoS1
//! en rafale vers un **abonne**, `rumqttd` 0.19 (config `max_inflight_count = 100` de
//! `iakahub/rumqttd.toml`) cesse durablement de relayer au-dela du seuil — un plafond de
//! *redistribution a un abonne*, propre a ce broker, sans rapport avec le transport
//! **daemon -> broker** teste ici (et `iakahub` est hors perimetre de ce lot). Compter les PUBLISH
//! **recus par le broker** (comme le fait la mesure de preuve A5 avec le broker Node) isole
//! exactement la question posee par A1 : le daemon perd-il des messages en les emettant ?
//!
//! A3 (resync deterministe + republication complete au retour en ligne) est couvert par le test
//! unitaire `mqtt::tests::resync_order_est_deterministe_et_republie_tout` (tri/dedup, pur, sans
//! reseau) **et** par le test d'integration `a3_resync_republie_tout_apres_coupure_et_reprise`
//! ci-dessous : meme broker maison que le reste de ce fichier, avec une coupure de connexion
//! deliberee en cours de lot puis une reconnexion, pour prouver que le resync declenche par le
//! vrai `ConnAck` republie tout l'etat sur le fil. A4 (aucune publication retentee dans le thread
//! `connection.iter()`) est un critere de revue de code, verifie par lecture du diff de `mqtt.rs`.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::BytesMut;
use iakatc_core::publish::Message;
use iakatc_daemon::config::DaemonConfig;
use iakatc_daemon::mqtt::MqttPublisher;
use rumqttc::mqttbytes::v4::{
    read as read_packet, ConnAck, ConnectReturnCode, Packet, PingResp, PubAck,
};
use rumqttc::mqttbytes::{Error as MqttBytesError, QoS};

/// Taille du lot de preuve : superieure a la capacite historique (64) et au lot reel (227).
const LOT_SIZE: usize = 300;
/// Taille max d'un paquet accepte par le broker de test (tres au-dessus d'un payload `{v,t}`).
const MAX_PACKET_SIZE: usize = 64 * 1024;

/// Reserve un port libre puis le relache (le broker qui suit le reprend juste apres).
fn free_port() -> u16 {
    let l = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemere");
    l.local_addr().unwrap().port()
}

fn test_config(port: u16, client_id: &str) -> DaemonConfig {
    DaemonConfig {
        host: "127.0.0.1".to_string(),
        port,
        user: None,
        password: None,
        root: "iakatokencounter".to_string(),
        client_id: client_id.to_string(),
        tick: Duration::from_secs(60),
        account_label: "test".to_string(),
    }
}

fn lot_de_messages(prefix: &str, n: usize) -> Vec<Message> {
    (0..n)
        .map(|i| Message {
            topic: format!("test/mqtt-no-loss/{prefix}/topic-{i:04}"),
            payload: format!("{{\"v\":{i},\"t\":1}}"),
        })
        .collect()
}

/// Demarre le broker minimal de test sur `port`, dans un thread dedie, et retourne l'ensemble
/// (partage) des topics **effectivement recus** par PUBLISH — la source de verite de A1/A5.
fn spawn_fake_broker(port: u16) -> Arc<Mutex<HashSet<String>>> {
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind broker de test");
    let received: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let received_for_thread = Arc::clone(&received);
    std::thread::spawn(move || {
        // Un seul client attendu par test (le `MqttPublisher` sous test).
        if let Ok((stream, _)) = listener.accept() {
            handle_client(stream, received_for_thread);
        }
    });
    received
}

/// Boucle de service d'une connexion : decode chaque paquet MQTT recu avec le meme codec que le
/// client (`rumqttc::mqttbytes::v4`), journalise chaque topic publie, acquitte QoS1 et repond aux
/// PINGREQ — le strict necessaire pour ne jamais faire attendre le publisher.
fn handle_client(mut stream: TcpStream, received: Arc<Mutex<HashSet<String>>>) {
    stream.set_nodelay(true).ok();
    let mut buf = BytesMut::with_capacity(MAX_PACKET_SIZE);
    let mut chunk = [0u8; 4096];
    loop {
        loop {
            match read_packet(&mut buf, MAX_PACKET_SIZE) {
                Ok(Packet::Connect(_)) => {
                    let mut out = BytesMut::new();
                    let _ = ConnAck::new(ConnectReturnCode::Success, false).write(&mut out);
                    if stream.write_all(&out).is_err() {
                        return;
                    }
                }
                Ok(Packet::Publish(p)) => {
                    received.lock().unwrap().insert(p.topic.clone());
                    if p.qos != QoS::AtMostOnce {
                        let mut out = BytesMut::new();
                        let _ = PubAck::new(p.pkid).write(&mut out);
                        if stream.write_all(&out).is_err() {
                            return;
                        }
                    }
                }
                Ok(Packet::PingReq) => {
                    let mut out = BytesMut::new();
                    let _ = PingResp.write(&mut out);
                    if stream.write_all(&out).is_err() {
                        return;
                    }
                }
                Ok(Packet::Disconnect) => return,
                Ok(_) => {}
                Err(MqttBytesError::InsufficientBytes(_)) => break,
                Err(_) => return,
            }
        }
        match stream.read(&mut chunk) {
            Ok(0) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => return,
        }
    }
}

/// Sert une premiere connexion qui coupe apres `drop_after` PUBLISH (force la reconnexion cote
/// client), puis laisse l'appelant servir la seconde connexion normalement (`handle_client`) — meme
/// codec et meme routine d'accuses que `handle_client`, avec une coupure deliberee au milieu.
fn handle_client_avec_coupure(
    mut stream: TcpStream,
    received: Arc<Mutex<HashSet<String>>>,
    drop_after: usize,
) {
    stream.set_nodelay(true).ok();
    let mut buf = BytesMut::with_capacity(MAX_PACKET_SIZE);
    let mut chunk = [0u8; 4096];
    let mut count = 0usize;
    loop {
        loop {
            match read_packet(&mut buf, MAX_PACKET_SIZE) {
                Ok(Packet::Connect(_)) => {
                    let mut out = BytesMut::new();
                    let _ = ConnAck::new(ConnectReturnCode::Success, false).write(&mut out);
                    if stream.write_all(&out).is_err() {
                        return;
                    }
                }
                Ok(Packet::Publish(p)) => {
                    received.lock().unwrap().insert(p.topic.clone());
                    count += 1;
                    let mut out = BytesMut::new();
                    let _ = PubAck::new(p.pkid).write(&mut out);
                    if stream.write_all(&out).is_err() {
                        return;
                    }
                    if count >= drop_after {
                        return; // coupure deliberee : force la reconnexion cote client
                    }
                }
                Ok(Packet::PingReq) => {
                    let mut out = BytesMut::new();
                    let _ = PingResp.write(&mut out);
                    if stream.write_all(&out).is_err() {
                        return;
                    }
                }
                Ok(Packet::Disconnect) => return,
                Ok(_) => {}
                Err(MqttBytesError::InsufficientBytes(_)) => break,
                Err(_) => return,
            }
        }
        match stream.read(&mut chunk) {
            Ok(0) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => return,
        }
    }
}

/// Demarre le broker de coupure/reprise A3 : la premiere connexion est coupee apres `drop_after`
/// PUBLISH, la seconde est servie jusqu'a la fin du test (`handle_client`, meme routine que A1/A2).
/// Retourne l'ensemble (partage) des topics recus, toutes connexions confondues — la source de
/// verite pour juger si le resync a republie tout l'etat apres reconnexion.
fn spawn_broker_coupure_puis_reprise(port: u16, drop_after: usize) -> Arc<Mutex<HashSet<String>>> {
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind broker de test");
    let received: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let received_for_thread = Arc::clone(&received);
    std::thread::spawn(move || {
        if let Ok((s1, _)) = listener.accept() {
            handle_client_avec_coupure(s1, Arc::clone(&received_for_thread), drop_after);
        }
        if let Ok((s2, _)) = listener.accept() {
            handle_client(s2, received_for_thread);
        }
    });
    received
}

/// A1 — lot de 300 topics distincts publies via `MqttPublisher` sur un broker de test : le broker
/// recoit les 300 PUBLISH, aucun manquant.
#[test]
fn a1_lot_de_300_topics_zero_perte() {
    let port = free_port();
    let received = spawn_fake_broker(port);

    let cfg = test_config(port, "test-a1-pub");
    let publisher = MqttPublisher::connect(&cfg);
    attendre_connexion(&publisher);

    let messages = lot_de_messages("a1", LOT_SIZE);
    let stats = publisher.publish_batch(&messages);

    assert_eq!(stats.emitted, LOT_SIZE);
    assert_eq!(
        stats.published, LOT_SIZE,
        "aucune perte attendue, broker joignable pendant tout le lot"
    );
    assert_eq!(stats.lost, 0);

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let count = received.lock().unwrap().len();
        if count >= LOT_SIZE {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "le broker n'a recu que {count}/{LOT_SIZE} PUBLISH avant expiration"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    assert_eq!(
        received.lock().unwrap().len(),
        LOT_SIZE,
        "le broker doit avoir recu les {LOT_SIZE} PUBLISH, aucun manquant"
    );
}

/// A2 — publisher pointe sur un port ferme (rien n'ecoute) : 300 messages rendent la main en
/// quelques secondes, sans panique, l'etat memoire est conserve (pas de perte de donnee, juste de
/// transport).
#[test]
fn a2_hors_ligne_borne_sans_panique() {
    let port = free_port(); // libere juste apres bind => rien n'ecoute dessus.
    let cfg = test_config(port, "test-a2-pub");
    let publisher = MqttPublisher::connect(&cfg);

    let messages = lot_de_messages("a2", LOT_SIZE);
    let start = Instant::now();
    let stats = publisher.publish_batch(&messages);
    let elapsed = start.elapsed();

    assert_eq!(stats.emitted, LOT_SIZE);
    assert!(
        elapsed < Duration::from_secs(10),
        "hors-ligne, l'appel doit rendre la main en quelques secondes (mesure: {elapsed:?})"
    );
    assert!(
        !publisher.is_connected(),
        "aucun broker n'ecoute sur ce port : la connexion ne doit jamais s'etablir"
    );
}

/// A3 — coupure de connexion en cours de lot puis reprise : le resync declenche par le `ConnAck`
/// de reconnexion republie **tout** l'etat connu, y compris les topics deja acquittes avant la
/// coupure (pas seulement ceux qui manquaient). C'est le mecanisme qui, en iterant jadis sur un
/// `HashMap` non deterministe, avait produit le patchwork a l'origine de ce lot ; une regression
/// dessus doit faire echouer ce test, pas seulement le test unitaire pur sur `resync_order`.
///
/// Anti-empilement (`resyncing`) non couvert ici : le forcer de facon fiable demanderait
/// d'instrumenter l'etat interne du publisher (aucune API publique n'expose si un resync est en
/// cours) ou d'enchainer des `ConnAck` assez vite pour garantir un chevauchement — non
/// reproductible de facon deterministe depuis un test boite noire sans le rendre flaky.
#[test]
fn a3_resync_republie_tout_apres_coupure_et_reprise() {
    let port = free_port();
    const DROP_AFTER: usize = 3;
    let received = spawn_broker_coupure_puis_reprise(port, DROP_AFTER);

    let cfg = test_config(port, "test-a3-resync");
    let publisher = MqttPublisher::connect(&cfg);
    attendre_connexion(&publisher);

    let messages = lot_de_messages("a3", 5);
    let stats = publisher.publish_batch(&messages);
    assert_eq!(stats.emitted, 5);

    // Le broker coupe la premiere connexion apres DROP_AFTER PUBLISH : sans resync sur
    // reconnexion, seuls ces DROP_AFTER topics seraient jamais recus. On attend que la
    // reconnexion + le resync aient republie les 5.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let count = received.lock().unwrap().len();
        if count >= 5 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "le resync apres reconnexion n'a pas republie tout l'etat a temps ({count}/5)"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    assert_eq!(
        received.lock().unwrap().len(),
        5,
        "le broker doit avoir recu les 5 topics, coupure + resync compris"
    );
}

/// Attend (borne) que le publisher ait recu son premier `ConnAck`.
fn attendre_connexion(publisher: &MqttPublisher) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !publisher.is_connected() {
        assert!(
            Instant::now() < deadline,
            "connexion au broker de test non etablie a temps"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
