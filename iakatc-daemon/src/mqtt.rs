//! mqtt — client MQTT (rumqttc) : publish **retained QoS 1**, reconnexion/backoff, republication
//! de l'etat courant au retour en ligne (contrat § 4/§ 6, D7).
//!
//! **Standalone / hors-ligne** : broker injoignable -> le publisher n'echoue pas ; il conserve en
//! memoire le dernier payload de chaque topic (etat retained a resynchroniser) et re-tente. A la
//! reconnexion (ConnAck), il republie tout l'etat. Le daemon continue de mesurer sans crash.
//!
//! **Transport sans perte (fix diagnostic)** : `try_publish` etant non bloquant, une file pleine
//! renvoie `ClientError::TryRequest` (a retenter) tandis qu'une file **deconnectee** renvoie
//! `ClientError::Request` (a ne surtout pas retenter de la meme facon). Le retry est **borne** en
//! tentatives et en temps (budget global par lot, `BATCH_RETRY_BUDGET`), et **jamais** execute
//! depuis le thread d'event-loop (`connection.iter()`) : ce thread est le seul a drainer le
//! channel, y attendre reviendrait a s'auto-bloquer (deadlock). Le resync sur `ConnAck` est donc
//! delegue a un thread court dedie, protege par un drapeau anti-empilement.

use rumqttc::{Client, ClientError, Event, MqttOptions, Packet, QoS};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::DaemonConfig;
use iakatc_core::publish::Message;

/// Capacite du channel interne rumqttc. Tres au-dessus du lot courant (227) et de sa croissance ;
/// la correction du transport repose sur le retry borne, pas sur cette capacite seule.
const CHANNEL_CAPACITY: usize = 1024;
/// Fenetre d'"inflight" QoS 1 (publications non encore acquittees). Le defaut rumqttc (100) fait
/// que l'event-loop **s'arrete d'envoyer** au-dela de ce seuil tant que TOUT l'inflight n'a pas
/// ete acquitte (stop-and-wait, pas de fenetre glissante) — mesure en test : avec le defaut, un
/// lot de 300 met plusieurs secondes a transiter (throttling, pas de perte). Alignee sur
/// `CHANNEL_CAPACITY` pour que rien ne pince le debit en dessous du lot courant et de sa
/// croissance.
const MQTT_INFLIGHT: u16 = CHANNEL_CAPACITY as u16;
/// Delai entre deux tentatives quand la file est pleine (`ClientError::TryRequest`).
const RETRY_DELAY: Duration = Duration::from_millis(20);
/// Nombre max de tentatives par message (~1 s au pire, 20ms x 50).
const MAX_RETRY_ATTEMPTS: u32 = 50;
/// Budget cumule de retry pour un lot (tick) entier : au-dela, on arrete de retenter pour ce lot,
/// on journalise le nombre de topics non publies, et on laisse l'etat memoire + le resync
/// rattraper. Garantit qu'aucun tick ne peut se figer, broker mort ou pas.
const BATCH_RETRY_BUDGET: Duration = Duration::from_secs(5);
/// Budget de retry du resync complet sur `ConnAck` — execute hors du thread d'event-loop, peut se
/// permettre d'etre plus genereux (republie potentiellement tout l'etat connu).
const RESYNC_RETRY_BUDGET: Duration = Duration::from_secs(10);

/// Dernier etat connu d'un topic : la valeur a publier (toujours mise a jour au tick courant) et
/// le fait qu'elle ait ete confirmee envoyee (poussee avec succes dans le channel rumqttc). Un
/// topic non envoye est reemis au tick suivant meme si sa valeur n'a pas change (invariant qui
/// rend la dedup differentielle future sure).
#[derive(Debug, Clone)]
struct TopicState {
    payload: String,
    sent: bool,
}

/// Statistiques honnetes d'un lot de publication (log de tick : emis / publies / perdus).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishStats {
    pub emitted: usize,
    pub published: usize,
    pub lost: usize,
}

/// Publisher MQTT resilient. Cloneable (Arc interne) — sans usage concurrent ici, mais partageable.
pub struct MqttPublisher {
    client: Client,
    /// Dernier etat connu par topic (source de verite a republier au resync).
    state: Arc<Mutex<HashMap<String, TopicState>>>,
    connected: Arc<AtomicBool>,
}

impl MqttPublisher {
    /// Cree le publisher et lance le thread d'event-loop (reconnexion automatique geree par
    /// rumqttc ; resync de l'etat delegue a un thread court sur chaque `ConnAck`).
    pub fn connect(cfg: &DaemonConfig) -> Self {
        let mut opts = MqttOptions::new(cfg.client_id.clone(), cfg.host.clone(), cfg.port);
        opts.set_keep_alive(Duration::from_secs(30));
        opts.set_inflight(MQTT_INFLIGHT);
        if let (Some(u), Some(p)) = (&cfg.user, &cfg.password) {
            opts.set_credentials(u.clone(), p.clone());
        }
        let (client, mut connection) = Client::new(opts, CHANNEL_CAPACITY);

        let state: Arc<Mutex<HashMap<String, TopicState>>> = Arc::new(Mutex::new(HashMap::new()));
        let connected = Arc::new(AtomicBool::new(false));
        // Anti-empilement du resync : un seul thread de resync a la fois, meme si les `ConnAck`
        // se succedent vite (flapping de connexion). Vecu uniquement par le thread d'event-loop
        // et les threads de resync qu'il spawn — jamais lu depuis l'exterieur du publisher.
        let resyncing = Arc::new(AtomicBool::new(false));

        // Thread d'event-loop : draine les notifications, gere connexion/reconnexion. **Ne publie
        // jamais rien lui-meme** — c'est le seul thread qui vide le channel ; y bloquer sur une
        // publication retentee serait un deadlock (cf. doc de module).
        {
            let client = client.clone();
            let state = Arc::clone(&state);
            let connected = Arc::clone(&connected);
            let resyncing = Arc::clone(&resyncing);
            std::thread::spawn(move || {
                for notification in connection.iter() {
                    match notification {
                        Ok(Event::Incoming(Packet::ConnAck(_))) => {
                            connected.store(true, Ordering::Relaxed);
                            spawn_resync(&client, &state, &connected, &resyncing);
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

    /// Publie un lot (un tick) de messages retained QoS 1, sans perte tant que le budget de retry
    /// n'est pas epuise. Chaque message est retente en cas de file pleine (borne en tentatives et
    /// en temps, budget partage par tout le lot) ; jamais bloquant indefiniment. Hors-ligne, on ne
    /// boucle pas : on memorise l'etat et on rend la main immediatement, le resync rattrapera.
    /// Retourne les compteurs honnetes emis/publies/perdus (jamais un mensonge de type
    /// `messages.len()`).
    pub fn publish_batch(&self, messages: &[Message]) -> PublishStats {
        let deadline = Instant::now() + BATCH_RETRY_BUDGET;
        let mut published = 0usize;
        let mut lost = 0usize;

        for m in messages {
            {
                let mut s = self.state.lock().unwrap();
                s.entry(m.topic.clone())
                    .and_modify(|e| {
                        e.payload = m.payload.clone();
                        e.sent = false;
                    })
                    .or_insert_with(|| TopicState {
                        payload: m.payload.clone(),
                        sent: false,
                    });
            }

            let ok = try_send_with_retry(
                &self.client,
                &self.connected,
                &m.topic,
                &m.payload,
                deadline,
            );
            if ok {
                if let Some(e) = self.state.lock().unwrap().get_mut(&m.topic) {
                    e.sent = true;
                }
                published += 1;
            } else {
                lost += 1;
            }
        }

        PublishStats {
            emitted: messages.len(),
            published,
            lost,
        }
    }

    /// Le broker est-il actuellement connecte ?
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

/// Tente de publier un message, avec retry borne si la file est pleine (`TryRequest`) — jamais si
/// le channel est deconnecte (`Request`, pas de retry). Hors-ligne (`!connected`), une seule
/// tentative non bloquante : pas de boucle, on rend la main tout de suite. Retourne `true` si le
/// message a ete effectivement pousse dans le channel rumqttc (pas une confirmation broker : le
/// QoS 1/retained est gere par rumqttc en interne une fois dans le channel).
fn try_send_with_retry(
    client: &Client,
    connected: &Arc<AtomicBool>,
    topic: &str,
    payload: &str,
    batch_deadline: Instant,
) -> bool {
    if !connected.load(Ordering::Relaxed) {
        return client
            .try_publish(
                topic.to_string(),
                QoS::AtLeastOnce,
                true,
                payload.as_bytes().to_vec(),
            )
            .is_ok();
    }

    let mut attempts: u32 = 0;
    loop {
        match client.try_publish(
            topic.to_string(),
            QoS::AtLeastOnce,
            true,
            payload.as_bytes().to_vec(),
        ) {
            Ok(()) => return true,
            Err(ClientError::TryRequest(_)) => {
                attempts += 1;
                if attempts >= MAX_RETRY_ATTEMPTS
                    || Instant::now() >= batch_deadline
                    || !connected.load(Ordering::Relaxed)
                {
                    eprintln!(
                        "[iakatc] MQTT publish abandonne pour {topic} (file pleine, {attempts} tentative(s))"
                    );
                    return false;
                }
                std::thread::sleep(RETRY_DELAY);
            }
            Err(e) => {
                eprintln!(
                    "[iakatc] MQTT publish echoue pour {topic} ({e}) — channel deconnecte, pas de retry"
                );
                return false;
            }
        }
    }
}

/// Ordonne l'etat connu de facon deterministe (tri alphabetique par topic) pour le resync — pure,
/// testable independamment du reseau. Republie **tout**, sans dedup : c'est le filet de rattrapage
/// d'un broker qui a perdu son retained.
fn resync_order(state: &HashMap<String, TopicState>) -> Vec<(String, String)> {
    let sorted: BTreeMap<&String, &TopicState> = state.iter().collect();
    sorted
        .into_iter()
        .map(|(topic, s)| (topic.clone(), s.payload.clone()))
        .collect()
}

/// Delegue le resync complet a un thread court dedie — **jamais** dans le thread d'event-loop
/// (cf. doc de module). Protege par `resyncing` pour ne pas empiler des threads si le lien bat de
/// l'aile (`ConnAck` repetes).
fn spawn_resync(
    client: &Client,
    state: &Arc<Mutex<HashMap<String, TopicState>>>,
    connected: &Arc<AtomicBool>,
    resyncing: &Arc<AtomicBool>,
) {
    if resyncing
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        // Un resync est deja en cours : ne pas empiler, celui en cours couvrira l'etat courant.
        return;
    }

    let client = client.clone();
    let state = Arc::clone(state);
    let connected = Arc::clone(connected);
    let resyncing = Arc::clone(resyncing);
    std::thread::spawn(move || {
        let snapshot = {
            let s = state.lock().unwrap();
            resync_order(&s)
        };
        let deadline = Instant::now() + RESYNC_RETRY_BUDGET;
        for (topic, payload) in snapshot {
            let ok = try_send_with_retry(&client, &connected, &topic, &payload, deadline);
            if ok {
                if let Some(e) = state.lock().unwrap().get_mut(&topic) {
                    e.sent = true;
                }
            } else {
                eprintln!("[iakatc] resync: abandon pour {topic}");
            }
        }
        resyncing.store(false, Ordering::Release);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resync_order_est_deterministe_et_republie_tout() {
        let mut state = HashMap::new();
        state.insert(
            "b/topic".to_string(),
            TopicState {
                payload: "2".to_string(),
                sent: true,
            },
        );
        state.insert(
            "a/topic".to_string(),
            TopicState {
                payload: "1".to_string(),
                sent: false,
            },
        );
        state.insert(
            "c/topic".to_string(),
            TopicState {
                payload: "3".to_string(),
                sent: true,
            },
        );

        let order = resync_order(&state);

        assert_eq!(
            order,
            vec![
                ("a/topic".to_string(), "1".to_string()),
                ("b/topic".to_string(), "2".to_string()),
                ("c/topic".to_string(), "3".to_string()),
            ],
            "le resync doit publier tout l'etat, tries par topic, sans filtrer sur `sent`"
        );
    }
}
