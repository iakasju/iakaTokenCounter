//! mqtt — client MQTT (rumqttc) : publish **retained QoS 1**, reconnexion/backoff, republication
//! de l'etat courant au retour en ligne (contrat § 4/§ 6, D7).
//!
//! **Standalone / hors-ligne** : broker injoignable -> le publisher n'echoue pas ; il conserve en
//! memoire le dernier payload de chaque topic (etat retained a resynchroniser) et re-tente. A la
//! reconnexion (ConnAck), il republie tout l'etat. Le daemon continue de mesurer sans crash.

use rumqttc::{Client, Event, MqttOptions, Packet, QoS};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::DaemonConfig;

/// Publisher MQTT resilient. Cloneable (Arc interne) — sans usage concurrent ici, mais partageable.
pub struct MqttPublisher {
    client: Client,
    /// Dernier payload retained par topic (source de verite a republier a la reconnexion).
    state: Arc<Mutex<HashMap<String, String>>>,
    connected: Arc<AtomicBool>,
}

impl MqttPublisher {
    /// Cree le publisher et lance le thread d'event-loop (reconnexion automatique geree par
    /// rumqttc ; republication de l'etat sur ConnAck).
    pub fn connect(cfg: &DaemonConfig) -> Self {
        let mut opts = MqttOptions::new(cfg.client_id.clone(), cfg.host.clone(), cfg.port);
        opts.set_keep_alive(Duration::from_secs(30));
        if let (Some(u), Some(p)) = (&cfg.user, &cfg.password) {
            opts.set_credentials(u.clone(), p.clone());
        }
        // Capacite de file d'attente : borne la memoire si le broker est absent longtemps.
        let (client, mut connection) = Client::new(opts, 64);

        let state: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let connected = Arc::new(AtomicBool::new(false));

        // Thread d'event-loop : draine les notifications, gere connexion/reconnexion.
        {
            let client = client.clone();
            let state = Arc::clone(&state);
            let connected = Arc::clone(&connected);
            std::thread::spawn(move || {
                for notification in connection.iter() {
                    match notification {
                        Ok(Event::Incoming(Packet::ConnAck(_))) => {
                            connected.store(true, Ordering::Relaxed);
                            // Republication de tout l'etat courant (resync des subscribers).
                            let snapshot: Vec<(String, String)> = {
                                let s = state.lock().unwrap();
                                s.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                            };
                            for (topic, payload) in snapshot {
                                let _ = client.try_publish(
                                    topic,
                                    QoS::AtLeastOnce,
                                    true,
                                    payload.into_bytes(),
                                );
                            }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            // Broker injoignable : on journalise, on marque deconnecte, backoff.
                            connected.store(false, Ordering::Relaxed);
                            eprintln!("[iakatc] MQTT hors-ligne ({e}) — nouvelle tentative...");
                            std::thread::sleep(Duration::from_secs(5));
                        }
                    }
                }
            });
        }

        MqttPublisher {
            client,
            state,
            connected,
        }
    }

    /// Publie (ou memorise) un message retained QoS 1. En cas d'echec (hors-ligne), l'etat est
    /// conserve et sera republié a la reconnexion — jamais de panique.
    pub fn publish(&self, topic: &str, payload: &str) {
        {
            let mut s = self.state.lock().unwrap();
            s.insert(topic.to_string(), payload.to_string());
        }
        let _ = self.client.try_publish(
            topic.to_string(),
            QoS::AtLeastOnce,
            true,
            payload.as_bytes().to_vec(),
        );
    }

    /// Le broker est-il actuellement connecte ?
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}
