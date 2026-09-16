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
//!
//! **Publication differentielle (B)** : un topic dont la valeur `v` n'a pas change **et** dont le
//! dernier envoi a ete confirme n'est pas republie au tick suivant. La dedup porte **uniquement**
//! sur `v` (jamais sur le payload complet ni sur `t`, qui change a chaque tick et rendrait toute
//! dedup naive inoperante). Un envoi non confirme (perte, hors-ligne) est **toujours** reemis au
//! tick suivant meme a `v` inchange (`TopicState::sent`, invariant deja pose par A). Le resync,
//! qu'il soit declenche par un `ConnAck` ou par le filet de securite periodique
//! (`MqttPublisher::force_resync`, appele tous les `PERIODIC_FULL_RESYNC_EVERY_N_TICKS` ticks
//! depuis `main.rs`), **ignore toujours la dedup** : il republie l'integralite de l'etat connu.
//!
//! **Le resync periodique n'est plus un simple filet de confort depuis B.** Avant B, un abonne dont
//! le broker (`rumqttd`) tronque les retained a la (re)connexion (au-dela de 100 messages, tirage
//! arbitraire dans un `HashMap`, `forward_retained` bascule a `false`) se retrouvait recomplete au
//! **tick suivant** malgre lui : le daemon republiait alors *tout* l'etat a *chaque* tick, dedup ou
//! pas. **Ce filet a disparu avec B.** Le resync periodique (cf. `PERIODIC_FULL_RESYNC_EVERY_N_TICKS`
//! dans `main.rs`) est donc devenu le **seul chemin de reparation** d'un abonne tronque — pas
//! seulement un garde-fou contre une divergence silencieuse du retained. **Ne pas espacer cette
//! periode pour « economiser du trafic »** sans en parler au decideur : l'espacer allonge d'autant
//! le temps pendant lequel un abonne tronque reste en etat incoherent (le patchwork qu'on vient de
//! corriger cote emission).

use rumqttc::{Client, ClientError, Event, MqttOptions, Packet, QoS};
use serde_json::Value;
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
/// rend la dedup differentielle sure, cf. doc de module § B).
#[derive(Debug, Clone)]
struct TopicState {
    payload: String,
    /// Valeur `v` extraite du dernier payload connu — jamais `t` (cf. doc de module). `None` si le
    /// payload n'a pu etre parse (ne devrait jamais arriver, le contrat garantit `{"v":...,"t":...}`) ;
    /// dans ce cas la dedup ne s'applique jamais (on republie par securite, cf. `should_publish`).
    value: Option<Value>,
    sent: bool,
}

/// Statistiques honnetes d'un lot de publication (log de tick : emis / publies / sautes / perdus).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishStats {
    pub emitted: usize,
    pub published: usize,
    /// Sautes par la dedup differentielle (B) : valeur `v` inchangee et dernier envoi confirme.
    pub skipped: usize,
    pub lost: usize,
}

/// Publisher MQTT resilient. Cloneable (Arc interne) — sans usage concurrent ici, mais partageable.
pub struct MqttPublisher {
    client: Client,
    /// Dernier etat connu par topic (source de verite a republier au resync).
    state: Arc<Mutex<HashMap<String, TopicState>>>,
    connected: Arc<AtomicBool>,
    /// Anti-empilement partage par le resync `ConnAck` et le resync periodique (`force_resync`) :
    /// un seul resync a la fois, quelle que soit son origine.
    resyncing: Arc<AtomicBool>,
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
            resyncing,
        }
    }

    /// Declenche un resync complet immediat, hors de tout `ConnAck` — le resync periodique (cf.
    /// doc de module § B) appele par `main.rs` tous les `PERIODIC_FULL_RESYNC_EVERY_N_TICKS`
    /// ticks. Reutilise exactement le meme mecanisme que le resync automatique (thread court
    /// dedie, anti-empilement partage) : republie tout l'etat connu, dedup ignoree — c'est
    /// **necessaire**, pas cosmetique : c'est le seul chemin qui repare un abonne dont le broker a
    /// tronque les retained a la connexion (cf. doc de module).
    pub fn force_resync(&self) {
        spawn_resync(&self.client, &self.state, &self.connected, &self.resyncing);
    }

    /// Publie un lot (un tick) de messages retained QoS 1, sans perte tant que le budget de retry
    /// n'est pas epuise, et **sans redondance** : un topic dont la valeur `v` n'a pas change et dont
    /// le dernier envoi a ete confirme est saute (dedup differentielle B, cf. doc de module).
    /// Chaque message effectivement envoye est retente en cas de file pleine (borne en tentatives
    /// et en temps, budget partage par tout le lot) ; jamais bloquant indefiniment. Hors-ligne, on
    /// ne boucle pas : on memorise l'etat et on rend la main immediatement, le resync rattrapera.
    /// Retourne les compteurs honnetes emis/publies/sautes/perdus (jamais un mensonge de type
    /// `messages.len()`).
    pub fn publish_batch(&self, messages: &[Message]) -> PublishStats {
        let deadline = Instant::now() + BATCH_RETRY_BUDGET;
        let mut published = 0usize;
        let mut skipped = 0usize;
        let mut lost = 0usize;

        for m in messages {
            let new_value = extract_v(&m.payload);

            let skip = {
                let mut s = self.state.lock().unwrap();
                let skip = !should_publish(s.get(&m.topic), &new_value);
                s.entry(m.topic.clone())
                    .and_modify(|e| {
                        e.payload = m.payload.clone();
                        e.value = new_value.clone();
                        if !skip {
                            e.sent = false;
                        }
                    })
                    .or_insert_with(|| TopicState {
                        payload: m.payload.clone(),
                        value: new_value.clone(),
                        sent: false,
                    });
                skip
            };

            if skip {
                skipped += 1;
                continue;
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
            skipped,
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

/// Extrait le champ `v` d'un payload `{"v":...,"t":...}` — jamais `t`, qui change a chaque tick et
/// rendrait toute dedup naive inoperante (cf. doc de module § B). `None` si le payload n'est pas un
/// JSON exploitable (ne devrait jamais arriver, le contrat garantit ce format) : un payload
/// illisible n'est jamais considere egal a une valeur precedente, on republie par securite.
fn extract_v(payload: &str) -> Option<Value> {
    serde_json::from_str::<Value>(payload)
        .ok()
        .and_then(|val| val.get("v").cloned())
}

/// Dedup differentielle (B) — pure, testable sans reseau. Un topic doit etre republie sauf s'il a
/// **deja** un etat connu, que ce dernier envoi a ete **confirme** (`sent`), et que la nouvelle
/// valeur `v` est **strictement egale** a la precedente. Un topic jamais vu, une valeur qui change,
/// ou un envoi precedent non confirme (perte, hors-ligne) republient toujours — c'est l'invariant
/// qui rend la dedup sure (B2) : on ne remplace jamais un bug de perte par un bug de silence.
fn should_publish(prev: Option<&TopicState>, new_value: &Option<Value>) -> bool {
    match prev {
        None => true,
        Some(p) => !(p.sent && new_value.is_some() && p.value == *new_value),
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
                value: Some(Value::from(2)),
                sent: true,
            },
        );
        state.insert(
            "a/topic".to_string(),
            TopicState {
                payload: "1".to_string(),
                value: Some(Value::from(1)),
                sent: false,
            },
        );
        state.insert(
            "c/topic".to_string(),
            TopicState {
                payload: "3".to_string(),
                value: Some(Value::from(3)),
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

    // --- B1/B2 : dedup differentielle (should_publish), pure, sans reseau ---

    #[test]
    fn extract_v_isole_la_valeur_et_ignore_t() {
        assert_eq!(extract_v(r#"{"v":1,"t":111}"#), Some(Value::from(1)));
        assert_eq!(
            extract_v(r#"{"v":1,"t":222}"#),
            Some(Value::from(1)),
            "un `t` different ne doit rien changer a la valeur extraite"
        );
        assert_eq!(
            extract_v(r#"{"v":"official","t":1}"#),
            Some(Value::from("official"))
        );
        assert_eq!(extract_v(r#"{"v":null,"t":1}"#), Some(Value::Null));
        assert_eq!(
            extract_v("pas du json"),
            None,
            "payload illisible => aucune valeur extraite, jamais compare egal"
        );
    }

    #[test]
    fn should_publish_republie_un_topic_jamais_vu() {
        assert!(should_publish(None, &Some(Value::from(1))));
    }

    /// B1(a) — valeur `v` inchangee et dernier envoi confirme => pas de republication, malgre `t`
    /// qui change (`t` n'entre meme pas dans cette fonction, cf. `extract_v`).
    #[test]
    fn should_publish_false_si_valeur_inchangee_et_envoi_confirme() {
        let prev = TopicState {
            payload: r#"{"v":1,"t":1}"#.to_string(),
            value: Some(Value::from(1)),
            sent: true,
        };
        assert!(
            !should_publish(Some(&prev), &Some(Value::from(1))),
            "meme v, envoi deja confirme => saute"
        );
    }

    /// B1(b) — valeur `v` differente => republication.
    #[test]
    fn should_publish_true_si_valeur_changee() {
        let prev = TopicState {
            payload: r#"{"v":1,"t":1}"#.to_string(),
            value: Some(Value::from(1)),
            sent: true,
        };
        assert!(
            should_publish(Some(&prev), &Some(Value::from(2))),
            "v different => republication"
        );
    }

    /// B2 — le critere le plus important : un envoi precedent **non confirme** (perte, hors-ligne)
    /// doit etre reemis au tick suivant meme si `v` n'a pas change. Sans cet invariant, la dedup
    /// remplacerait un bug de perte par un bug de silence permanent.
    #[test]
    fn should_publish_true_si_envoi_precedent_non_confirme_meme_a_valeur_inchangee() {
        let prev = TopicState {
            payload: r#"{"v":1,"t":1}"#.to_string(),
            value: Some(Value::from(1)),
            sent: false,
        };
        assert!(
            should_publish(Some(&prev), &Some(Value::from(1))),
            "envoi precedent non confirme => reemission meme a v inchange"
        );
    }

    #[test]
    fn should_publish_true_si_nouvelle_valeur_illisible() {
        let prev = TopicState {
            payload: r#"{"v":1,"t":1}"#.to_string(),
            value: Some(Value::from(1)),
            sent: true,
        };
        assert!(
            should_publish(Some(&prev), &None),
            "valeur illisible => jamais consideree egale, on republie par securite"
        );
    }
}
