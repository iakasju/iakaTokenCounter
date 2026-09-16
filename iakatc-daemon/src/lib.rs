//! iakatc-daemon (lib) — expose `config` et `mqtt` pour les tests d'integration
//! (`iakatc-daemon/tests/`). Le binaire (`main.rs`) consomme cette lib ; aucun changement de
//! comportement, seulement la surface testable de `mqtt.rs` sans broker externe.

pub mod config;
pub mod mqtt;
