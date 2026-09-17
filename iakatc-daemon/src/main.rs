//! iakatc-daemon — daemon de mesure headless (D7) + sous-commande `statusline-capture` (D4).
//!
//! Deux modes selon le 1er argument :
//! - `iakatc-daemon statusline-capture` : lit le JSON statusline sur stdin, persiste le quota,
//!   re-emet une ligne minimale (ne casse jamais la statusline).
//! - `iakatc-daemon` (sans argument) : boucle de tick — re-scan complet des logs Claude+Codex,
//!   fusion quota, publication MQTT retained code/value selon `contrat-mqtt-conso.md`.
//!
//! Hors-ligne : broker injoignable -> mesure + journalisation + retente + republication a la
//! reconnexion. Le daemon ne crashe pas.

mod statusline;

use std::collections::HashMap;

use iakatc_core::measure::cache::{scan_claude_measurements_cached, ScanCache};
use iakatc_core::measure::{claude, codex, Measurement, Provider};
use iakatc_core::now_epoch_s;
use iakatc_core::publish::contract;
use iakatc_core::quota::{config as qconfig, merge, resolve_home};

use iakatc_daemon::config::DaemonConfig;
use iakatc_daemon::mqtt::MqttPublisher;

/// Version publiee dans `meta/daemon/version` (suit la version du crate).
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Resync complet periodique meme connecte, pour borner a ~10 min toute divergence silencieuse
/// entre l'etat du daemon et le retained du broker (la dedup differentielle ne republie plus un
/// topic inchange, donc un retained perdu sans coupure TCP visible ne serait sinon jamais rattrape
/// avant le prochain changement de valeur). Constante nommee, pas de configuration nouvelle (cf.
/// instruction § B, etape 9).
///
/// **Ce n'est plus un simple filet de confort.** `rumqttd` tronque definitivement les retained
/// qu'il renvoie a un abonne qui se (re)connecte au-dela de 100 messages (tirage arbitraire dans
/// un `HashMap`, pas une file qui s'ecoule) — un abonne peut donc recevoir un etat partiel et
/// incoherent (ex. `remaining_pct` sans son `confidence`, le patchwork que ce lot corrige cote
/// emission). Avant B, ce defaut se reparait seul en un tick (le daemon republiait *tout* l'etat a
/// *chaque* tick, dedup ou pas) ; depuis B, **ce resync periodique est le seul chemin qui repare un
/// abonne tronque**. **Ne pas espacer cette periode « pour economiser du trafic » sans en parler au
/// decideur** : l'espacer allonge d'autant le temps pendant lequel un abonne tronque reste
/// incoherent.
const PERIODIC_FULL_RESYNC_EVERY_N_TICKS: u64 = 10;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("statusline-capture") {
        // La sous-commande ne doit JAMAIS casser la statusline : toujours code 0.
        let cfg = DaemonConfig::from_env();
        statusline::run(&cfg.account_label);
        return;
    }
    run_daemon();
}

/// Boucle de vie du daemon : tick periodique, mesure -> fusion -> publication retained.
fn run_daemon() {
    let cfg = DaemonConfig::from_env();
    eprintln!(
        "[iakatc] daemon v{VERSION} — broker {}:{}, racine '{}', tick {}s",
        cfg.host,
        cfg.port,
        cfg.root,
        cfg.tick.as_secs()
    );
    let publisher = MqttPublisher::connect(&cfg);

    // Memo par fichier (mtime, taille) du scan mesure Claude (D5) : vit ici, d'un tick a l'autre,
    // pour tout le cycle de vie du process. Un redemarrage repart d'un memo vide (premier tick =
    // scan complet). Cf. `iakatc-core::measure::cache` pour pourquoi c'est correct.
    let mut scan_cache = ScanCache::new();

    let mut tick_count: u64 = 0;
    loop {
        tick(&cfg, &publisher, &mut scan_cache);
        tick_count += 1;
        if tick_count.is_multiple_of(PERIODIC_FULL_RESYNC_EVERY_N_TICKS) {
            // Filet de securite B : republie tout l'etat connu, dedup ignoree — cf. doc de la
            // constante et `MqttPublisher::force_resync`.
            publisher.force_resync();
        }
        std::thread::sleep(cfg.tick);
    }
}

/// Un tick : re-scan des logs (recalcul depuis le disque, pas d'increment memoire — le memo D5
/// evite seulement de RELIRE un fichier inchange, il ne remplace jamais le calcul), fusion du
/// quota, construction des messages du contrat, publication retained.
fn tick(cfg: &DaemonConfig, publisher: &MqttPublisher, scan_cache: &mut ScanCache) {
    let now = now_epoch_s();

    // --- Mesure conso (Claude + Codex) ---
    let mut measurements: Vec<Measurement> = Vec::new();
    if let Some(dir) = claude::claude_projects_dir() {
        // Variante memoisee (D5) : seuls les fichiers dont (mtime, taille) a change depuis le
        // tick precedent sont relus + re-parses.
        measurements.extend(scan_claude_measurements_cached(&dir, scan_cache));
    }
    let mut codex_rl = Vec::new();
    if let Some(dir) = codex::codex_sessions_dir() {
        let (m, rl) = codex::scan_codex(&dir);
        measurements.extend(m);
        codex_rl = rl;
    }

    // --- Fusion quota (Claude statusline + estimation) ---
    let quota_cfg = resolve_home()
        .map(|home| qconfig::load_config(&home))
        .unwrap_or_default();
    let quota_files = resolve_home()
        .map(|home| iakatc_core::quota::store::load_quota_files(&home))
        .unwrap_or_default();

    let mut measured_by_provider: HashMap<String, u64> = HashMap::new();
    for provider in [Provider::Claude, Provider::Codex] {
        let used = iakatc_core::aggregate::used_tokens_by_provider(&measurements, provider);
        if used > 0 {
            measured_by_provider.insert(provider.code().to_string(), used);
        }
    }

    let mut reservoirs = merge::merge(&quota_files, &quota_cfg, &measured_by_provider, now);
    // Quota Codex best-effort (D3) : n'ajoute rien tant que la fenetre ne mappe pas 5h/7d.
    let codex_used = measured_by_provider.get("codex").copied();
    reservoirs.extend(merge::codex_reservoirs(
        &cfg.account_label,
        &codex_rl,
        codex_used,
        now,
    ));

    // --- Construction + publication des messages du contrat ---
    let broker_connected = publisher.is_connected();
    let messages = contract::tick_messages(
        &cfg.root,
        &measurements,
        &reservoirs,
        &quota_cfg,
        "up",
        now,
        broker_connected,
        VERSION,
        now,
    );
    let stats = publisher.publish_batch(&messages);
    warn_on_retained_backlog(&cfg.root, &messages);
    eprintln!(
        "[iakatc] tick {} — {} emis / {} publies / {} inchanges (sautes) / {} perdus (broker {})",
        now,
        stats.emitted,
        stats.published,
        stats.skipped,
        stats.lost,
        if broker_connected {
            "connecte"
        } else {
            "hors-ligne"
        }
    );
}

/// Garde-fou C6 (garde-plafond-retained-broker.md) : avertit, filtre par filtre, quand le nombre
/// de topics du **lot complet du contrat** (`messages` = `tick_messages`, pas `stats.published`)
/// couverts par un filtre de consommateur atteint `RETAINED_BACKLOG_ALERT`. Compter sur le lot
/// complet et non sur ce qui a ete effectivement publie ce tick est delibere : avec la dedup
/// differentielle (lot B), un tick ne republie plus tous les topics inchanges — compter sur
/// `stats.published` rendrait ce garde-fou aveugle sans que personne ne le voie (cf. § Risques de
/// l'instruction). Silencieux sous le seuil : aucun log ajoute tant qu'aucun filtre n'approche le
/// plafond.
fn warn_on_retained_backlog(root: &str, messages: &[iakatc_core::publish::Message]) {
    let filters = contract::consumer_filters(root);
    for (filter, count) in contract::backlog_by_filter(messages, &filters) {
        if count >= contract::RETAINED_BACKLOG_ALERT {
            eprintln!(
                "[iakatc] ATTENTION retained : le filtre '{filter}' couvre {count} topics (plafond dur {}) — un abonne qui se (re)connecte au-dela de ce plafond ne recevra qu'une partie de son etat initial",
                contract::RETAINED_FANOUT_CEILING
            );
        }
    }
}
