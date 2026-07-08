//! Test d'integration in-process du broker embarque (critere D1) :
//! demarrer iakahub::broker sur `127.0.0.1`, publier un message **retained** avec un client
//! `rumqttc`, puis, via un **second client** qui s'abonne, **recevoir immediatement** la valeur
//! retained — sans aucun broker externe.
//!
//! Le port est choisi libre au moment du test (bind ephemere puis relache) pour eviter les
//! collisions et rester deterministe.

use std::net::TcpListener;
use std::time::{Duration, Instant};

use rumqttc::{Client, Event, MqttOptions, Packet, QoS};

/// Reserve un port libre puis le relache : le broker le reprendra juste apres.
fn free_port() -> u16 {
    let l = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemere");
    l.local_addr().unwrap().port()
}

#[test]
fn broker_local_retained_roundtrip() {
    let port = free_port();
    let addr = iakahub::broker::start(port).expect("le broker local doit demarrer");
    assert_eq!(addr.port(), port);

    let topic = "iakatokencounter/meta/daemon/state/current";
    let payload = br#"{"v":"up","t":1751894400}"#;

    // --- Publisher : publie un retained QoS 1 (comme le fait le daemon). ---
    {
        let mut opts = MqttOptions::new("iakahub-test-pub", "127.0.0.1", port);
        opts.set_keep_alive(Duration::from_secs(5));
        // Creds factices : le broker anonyme les ignore (fait verifie).
        opts.set_credentials("iakahub", "local");
        let (pub_client, mut pub_conn) = Client::new(opts, 16);

        pub_client
            .publish(topic, QoS::AtLeastOnce, true, payload.to_vec())
            .expect("publish retained");

        // Pompe l'event-loop jusqu'a l'acquittement du publish (PubAck), borne dans le temps.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut acked = false;
        for notif in pub_conn.iter() {
            if let Ok(Event::Incoming(Packet::PubAck(_))) = notif {
                acked = true;
                break;
            }
            if Instant::now() > deadline {
                break;
            }
        }
        assert!(acked, "le publish retained doit etre acquitte par le broker");
        pub_client.disconnect().ok();
    }

    // --- Subscriber : un NOUVEAU client recoit immediatement le retained. ---
    let mut opts = MqttOptions::new("iakahub-test-sub", "127.0.0.1", port);
    opts.set_keep_alive(Duration::from_secs(5));
    let (sub_client, mut sub_conn) = Client::new(opts, 16);
    sub_client
        .subscribe(topic, QoS::AtLeastOnce)
        .expect("subscribe");

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut received: Option<Vec<u8>> = None;
    for notif in sub_conn.iter() {
        if let Ok(Event::Incoming(Packet::Publish(p))) = notif {
            received = Some(p.payload.to_vec());
            break;
        }
        if Instant::now() > deadline {
            break;
        }
    }
    sub_client.disconnect().ok();

    assert_eq!(
        received.as_deref(),
        Some(&payload[..]),
        "le second client doit recevoir la valeur retained publiee"
    );
}

#[test]
fn port_occupe_est_signale_sans_paniquer() {
    // Occupe un port puis demande a iakahub de demarrer dessus -> erreur claire (fail-fast D5).
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = listener.local_addr().unwrap().port();
    let res = iakahub::broker::start(port);
    assert!(res.is_err(), "un port occupe doit etre signale par une erreur");
    drop(listener);
}
