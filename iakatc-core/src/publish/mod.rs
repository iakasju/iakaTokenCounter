//! publish — mapping des mesures vers le format sur le fil du `contrat-mqtt-conso.md`.
//!
//! Un [`Message`] = un **couple (topic-code, payload scalaire `{v,t}`)**. Aucun objet composite
//! n'est jamais produit (contrat § 0/§ 3). Le module `contract` construit ces messages a partir
//! des agregats (`aggregate`), du `Reservoir` (`quota::merge`), de la config et de l'etat du daemon.

pub mod contract;

/// Un message pret a publier : le **topic** (= le code) et le **payload** JSON (`{"v":...,"t":...}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub topic: String,
    pub payload: String,
}
