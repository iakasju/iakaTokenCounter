//! iakahub v0 — backbone local du poste : **broker MQTT embarque** (`rumqttd`, `127.0.0.1`,
//! anonyme) + **orchestration/supervision** du measure daemon `iakatc-daemon`.
//!
//! Voir `specs/instructions/feature-iakahub.md` (decisions D1..D5) et `specs/contrat-mqtt-conso.md`.
//! Les modules sont exposes en bibliotheque pour etre testables (round-trip broker in-process,
//! resolveur de chemin, logique de supervision) ; le binaire `iakahub` (`main.rs`) les orchestre.

pub mod broker;
pub mod shutdown;
pub mod supervisor;
