//! Tests d'integration de la publication differentielle (B) — criteres B1/B3 de
//! `specs/instructions/fix-mqtt-transport-pertes.md`.
//!
//! Meme famille de broker MQTT 3.1.1 minimal que `tests/mqtt_no_loss.rs` (memes raisons : eviter le
//! plafond de redistribution *broker -> abonne* de `rumqttd`, hors sujet ici), mais qui journalise
//! **chaque** PUBLISH recu, avec doublons, dans l'ordre — `spawn_fake_broker` de `mqtt_no_loss.rs`
//! ne garde qu'un ensemble deduplique (suffisant pour A1/A3), alors qu'ici il faut precisement
//! distinguer « publie une fois » de « republie », ce que seul un compte par topic permet de
//! prouver.
//!
//! B2 (un envoi non confirme est reemis au tick suivant meme a `v` inchange) est couvert par le
//! test unitaire pur `mqtt::tests::should_publish_true_si_envoi_precedent_non_confirme_meme_a_valeur_inchangee`
//! (`iakatc-daemon/src/mqtt.rs`) : c'est le critere le plus important de B, et il est deterministe
//! sans reseau — le reproduire fidelement ici demanderait de forcer une perte de transport
//! artificielle, ce que `try_send_with_retry` ne permet pas d'observer depuis l'exterieur sans
//! flakiness.

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
use rumqttc::mqttbytes::Error as MqttBytesError;

const MAX_PACKET_SIZE: usize = 64 * 1024;

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

/// Demarre un broker de test qui journalise **chaque** PUBLISH recu (avec doublons, dans l'ordre)
/// — necessaire pour distinguer « publie une fois » de « republie » sur un meme topic.
fn spawn_counting_broker(port: u16) -> Arc<Mutex<Vec<String>>> {
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind broker de test");
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let received_for_thread = Arc::clone(&received);
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            handle_client(stream, received_for_thread);
        }
    });
    received
}

fn handle_client(mut stream: TcpStream, received: Arc<Mutex<Vec<String>>>) {
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
                    received.lock().unwrap().push(p.topic.clone());
                    let mut out = BytesMut::new();
                    let _ = PubAck::new(p.pkid).write(&mut out);
                    if stream.write_all(&out).is_err() {
                        return;
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

fn count_topic(received: &Arc<Mutex<Vec<String>>>, topic: &str) -> usize {
    received
        .lock()
        .unwrap()
        .iter()
        .filter(|t| t.as_str() == topic)
        .count()
}

fn attendre_total(received: &Arc<Mutex<Vec<String>>>, n: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if received.lock().unwrap().len() >= n {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "le broker n'a pas recu {n} PUBLISH a temps (recu: {})",
            received.lock().unwrap().len()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn attendre_occurrences(received: &Arc<Mutex<Vec<String>>>, topic: &str, n: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if count_topic(received, topic) >= n {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "le topic {topic} n'a pas ete recu {n} fois a temps (recu: {})",
            count_topic(received, topic)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

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

/// B1 — un topic dont la valeur `v` n'a pas change (malgre un `t` different) et dont l'envoi
/// precedent a ete confirme n'est **pas** republie ; un topic dont la valeur change est republie.
/// Preuve reseau (compte les PUBLISH reellement recus par le broker), en complement du test unitaire
/// pur `mqtt::tests::should_publish_*`.
#[test]
fn b1_dedup_par_valeur_ignore_t_republie_si_change() {
    let port = free_port();
    let received = spawn_counting_broker(port);

    let cfg = test_config(port, "test-b1-dedup");
    let publisher = MqttPublisher::connect(&cfg);
    attendre_connexion(&publisher);

    let topic_x = "test/mqtt-dedup/b1/x".to_string();
    let topic_y = "test/mqtt-dedup/b1/y".to_string();

    let batch1 = vec![
        Message {
            topic: topic_x.clone(),
            payload: r#"{"v":1,"t":1}"#.to_string(),
        },
        Message {
            topic: topic_y.clone(),
            payload: r#"{"v":5,"t":1}"#.to_string(),
        },
    ];
    let stats1 = publisher.publish_batch(&batch1);
    assert_eq!(
        stats1.published, 2,
        "premier tick : rien de connu, tout publie"
    );
    assert_eq!(stats1.skipped, 0);
    attendre_total(&received, 2);

    // Deuxieme tick : X inchange (meme v, t different), Y change de valeur.
    let batch2 = vec![
        Message {
            topic: topic_x.clone(),
            payload: r#"{"v":1,"t":2}"#.to_string(),
        },
        Message {
            topic: topic_y.clone(),
            payload: r#"{"v":6,"t":2}"#.to_string(),
        },
    ];
    let stats2 = publisher.publish_batch(&batch2);
    assert_eq!(
        stats2.published, 1,
        "seul Y (valeur changee) doit etre publie"
    );
    assert_eq!(
        stats2.skipped, 1,
        "X (valeur inchangee) doit etre saute malgre t different"
    );

    attendre_occurrences(&received, &topic_y, 2);
    // Laisse une fenetre pour qu'un envoi en trop de X, s'il y en avait un, ait le temps d'arriver.
    std::thread::sleep(Duration::from_millis(300));

    assert_eq!(
        count_topic(&received, &topic_x),
        1,
        "X ne doit avoir ete publie qu'une seule fois (valeur jamais changee)"
    );
    assert_eq!(
        count_topic(&received, &topic_y),
        2,
        "Y doit avoir ete publie deux fois (valeur changee au 2e tick)"
    );
}

/// B3 — le resync republie **tout** l'etat connu, y compris un topic deduplique lors des ticks
/// normaux : un abonne qui se connecte apres plusieurs ticks dedupliques recoit bien la valeur
/// courante via le retained. Ce test appelle directement `MqttPublisher::force_resync` — c'est
/// **exactement** l'appel que fait `main.rs` pour le resync periodique (meme fonction, aucune
/// variante), et le meme mecanisme interne (`spawn_resync`) que le resync automatique sur
/// `ConnAck` : prouver que `force_resync` ignore la dedup prouve donc les deux. Couvre le point
/// souleve par le decideur sur la troncature des retained par `rumqttd` (> 100 messages a la
/// connexion) : depuis B, ce resync periodique est le **seul** chemin qui recomplete un abonne
/// tronque (cf. doc de module de `mqtt.rs` et de `PERIODIC_FULL_RESYNC_EVERY_N_TICKS` dans
/// `main.rs`) — ce test verifie qu'il ne filtre bien jamais sur la dedup.
#[test]
fn b3_resync_ignore_la_dedup_et_republie_tout() {
    let port = free_port();
    let received = spawn_counting_broker(port);

    let cfg = test_config(port, "test-b3-resync-dedup");
    let publisher = MqttPublisher::connect(&cfg);
    attendre_connexion(&publisher);

    let topic = "test/mqtt-dedup/b3/x".to_string();
    let batch1 = vec![Message {
        topic: topic.clone(),
        payload: r#"{"v":42,"t":1}"#.to_string(),
    }];
    publisher.publish_batch(&batch1);
    attendre_total(&received, 1);

    // Meme valeur au tick suivant : republication normalement sautee par la dedup.
    let batch2 = vec![Message {
        topic: topic.clone(),
        payload: r#"{"v":42,"t":2}"#.to_string(),
    }];
    let stats2 = publisher.publish_batch(&batch2);
    assert_eq!(stats2.skipped, 1);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        count_topic(&received, &topic),
        1,
        "dedup active : le broker n'a recu qu'une seule publication a ce stade"
    );

    // Resync force (meme mecanisme que le resync periodique du filet de securite) : doit republier
    // malgre la valeur inchangee et le dernier envoi confirme.
    publisher.force_resync();
    attendre_occurrences(&received, &topic, 2);
}
