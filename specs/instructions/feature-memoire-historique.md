# Instruction : Mémoire de l'historique (quota historisé + rollups quotidiens)

> Rédigé par Gandalf (P1 — cadrage). Consommé par Gimli (Claude Code) comme instruction de
> travail. Doc en français, code/identifiants en anglais.
> **Lot L1** du jalon arbitré par le décideur le 2026-09-17 (L0 + L1 + L2, 6 j).
> Proposition d'origine et mesures : `specs/instructions/proposition-analytics-riche.md`.
>
> **Périmètre : app tray Tauri (`src-tauri/`) + `iatc-core` en lecture.** Aucune modification du
> contrat MQTT, du daemon, ni des visualisations.

---

## ⚠️ Priorité de séquence — à lire avant de planifier

**Ce lot doit être allumé le plus tôt possible, même si ce qu'il alimente ne s'affiche que plus
tard.** C'est le seul lot du jalon dont **la valeur dépend du temps écoulé depuis sa mise en
route** :

- l'historique de quota **n'existe pas rétroactivement** : il commence le jour où on l'allume ;
- et pendant ce temps, **la purge de Claude Code ronge par l'autre bout** — les transcripts de
  plus de 30 jours sont supprimés au démarrage, `subagents/` compris.

Chaque semaine de report est une semaine d'histoire définitivement perdue. **On plante l'arbre
avant d'avoir faim.** Ce lot peut être livré **sans aucune interface** : il écrit, il ne montre
rien. C'est voulu.

## Contexte

Deux questions du décideur n'ont **aucune réponse possible** avec les sources actuelles :

- **« Vais-je tenir jusqu'au rechargement ? »** — le quota vient de la statusline et transite en
  MQTT *retained* : c'est une valeur courante, jamais une série. Le broker ne persiste rien ; un
  redémarrage repart d'un état vide.
- **« Quel compte est le plus sollicité ? »** — les transcripts ne portent **aucun identifiant de
  compte** (vérifié : `userID`, `accountUuid`, `organizationUuid` → zéro occurrence). L'axe compte
  n'existe que du côté quota.

À quoi s'ajoute une limite de profondeur que personne n'avait vue : **ce que la vue appelle
*all-time* est en réalité une fenêtre glissante d'environ 30 jours.** La profondeur réelle mesurée
est de 45 jours, et elle ne grandira pas.

Ce lot constitue donc la **mémoire propre de l'application** : deux flux persistés localement, qui
survivent à la fermeture de la fenêtre, au redémarrage du tray, et à la purge de la source.

## Faits vérifiés (état de l'art, 2026-09-17)

- **Claude Code purge les transcripts de plus de 30 jours**, au démarrage, via
  `cleanupPeriodDays` (défaut **30**) — session `.jsonl` **et** fichiers `subagents/`. Sources en
  fin de document.
- **Profondeur réelle constatée** : 45 jours (2026-08-03 → 2026-09-17), 700 Mo, 584 fichiers.
  **Croissance rapide** : 284 Mo en août, 416 Mo en septembre.
- **Le quota est déjà reçu par le tray** : `mqtt_sub` alimente `state.rs::ReservoirStore` via le
  filtre `{root}/all/ia/+/+/quota/#` (`publish/contract.rs::consumer_filters`). Les codes utiles
  sont déjà décodés en `WindowState` : `used_pct`, `remaining_pct`, `used_tokens`, `resets_at`,
  `captured_at`, `confidence`, `source`, `updated_at`.
- **Le patron de persistance existe, écrit et testé** : `src-tauri/src/memory.rs` historise la RAM
  en JSONL borné — `append_sample:107`, `read_history:119` (tri croissant, lignes corrompues
  ignorées), `compact:136` (réécriture atomique via `.tmp` + `rename`), rétention glissante,
  thread détaché qui **ne panique jamais**, chemin résolu sous `app_data_dir`.
- **Le tray tourne en permanence en headless** (app `Accessory` sur macOS, `skipTaskbar` ailleurs) :
  il y a donc un process hôte disponible en continu pour échantillonner, **indépendamment de
  l'ouverture de la fenêtre** — c'est exactement ce qui a rendu la courbe RAM persistante.

## Ce qui existe (à réutiliser)

| Élément | Où | Rôle pour ce lot |
|---|---|---|
| **Persistance JSONL bornée complète** | `src-tauri/src/memory.rs:107`, `:119`, `:136` | **Le patron à décalquer** : append, lecture triée, compaction atomique |
| Thread de fond détaché | `memory.rs::start_sampler:175`, lancé au `setup` (`lib.rs`) | Patron du sampler de quota |
| État quota déjà reçu et décodé | `src-tauri/src/state.rs::ReservoirStore`, `WindowState:44` | **La source du flux quota — rien à brancher** |
| Détection de changement d'état | `state.rs::set_code:58` (retourne `true` si l'état change) | Sert à n'écrire que sur changement |
| Chemin de données de l'app | `app_data_dir` résolu au `setup`, `MemoryLog::in_dir:167` | Même répertoire pour les nouveaux fichiers |
| Scans de conso (après L0) | `iakatc-core/src/measure/claude.rs` | Source des rollups quotidiens |
| Commande read-only modèle | `src-tauri/src/history.rs::get_history:107` | Patron des nouvelles commandes |

## Décision

### D1 — On rouvre D2 de `feature-app-analytics.md`, **par la clause que D2 posait lui-même**

Le cadrage d'origine avait écarté tout store local, en inscrivant noir sur blanc sa condition de
réouverture : *« utile seulement le jour où l'on voudra une granularité infra-journalière **ou une
source qui s'efface du disque** ; ce n'est pas le cas des JSONL »*.

**C'est désormais le cas des JSONL.** La prémisse factuelle est tombée ; la clause s'applique. Ce
lot **ne renverse pas D2**, il exécute ce que D2 prévoyait. Arbitré par le décideur le 2026-09-17.

**Limite que D2 imposait et qui reste vraie** : le store **ne recalcule rien**. Il ne devient pas
un second moteur de mesure. Il **conserve** un résultat que la source va détruire — rien de plus.

### D2 — Deux fichiers, deux natures, deux rétentions

| Fichier | Contenu | Cadence | Rétention |
|---|---|---|---|
| `quota-history.jsonl` | Un point par `(provider, account, window)` : `used_pct`, `remaining_pct`, `resets_at`, `confidence` | Échantillon **toutes les 5 min**, écrit **si la valeur a changé** ou si **plus d'une heure** s'est écoulée depuis le dernier point de cette série | **90 jours** glissants |
| `daily-rollups.jsonl` | Un enregistrement par `(jour, projet, provider, agent)` : tokens des deux grandeurs de L0 | Recalculé à chaque ouverture de la vue et une fois par jour | **Sans limite** |

**Pourquoi deux fichiers et pas un** : ils n'ont ni la même cadence, ni la même clé, ni la même
rétention, ni la même source. Les mêler imposerait un schéma commun artificiel.

**Pourquoi 5 min + écriture sur changement** pour le quota : la statusline n'est de toute façon
rafraîchie qu'à l'usage ; échantillonner plus fin n'ajoute aucune information et grossit le
fichier. L'écriture conditionnelle évite des milliers de points identiques pendant les périodes
d'inactivité, **et le point horaire forcé garde les plateaux visibles** (sans lui, une courbe
reconstituée par interpolation mentirait sur la durée des paliers).

**Pourquoi 90 jours pour le quota** : la plus longue fenêtre du contrat est 30 jours (plan free
Codex). Trois cycles suffisent largement à lire une tendance ; au-delà, la donnée n'éclaire plus
aucune décision. Volume attendu : 5 réservoirs × ~300 points/jour au pire ≈ **quelques Mo sur
90 jours**.

**Pourquoi « sans limite » pour les rollups** : c'est la réponse directe à la purge à 30 jours.
Volume attendu : ~44 projets × 2 agents × 365 jours ≈ **32 000 lignes par an**, quelques Mo.
Une rétention serait une complication sans bénéfice.

> **Tranché sans remonter au décideur** (sa question Q4 portait sur la profondeur) : **rollups sans
> limite, quota à 90 jours, et on ne touche PAS à `cleanupPeriodDays`.** Relever la purge ferait
> croître les transcripts d'environ **400 Mo par mois** sur le poste, pour une donnée brute dont on
> n'a besoin que sous forme agrégée. Les rollups donnent la profondeur pour quelques Mo. Si le
> décideur veut malgré tout garder les transcripts bruts, c'est un réglage de son `settings.json`,
> pas une décision de ce projet.

### D3 — Le sampler de quota vit dans le tray, pas dans le daemon

**Retenu** : un thread détaché dans le process tray, décalqué de `memory.rs::start_sampler`, qui
lit l'état déjà agrégé par `ReservoirStore` et l'appende.

**Pourquoi le tray et pas le daemon** : (a) l'état quota **y est déjà**, fusionné et décodé — aucun
abonnement, aucun parsing, aucune source nouvelle à brancher ; (b) c'est là que le patron de
persistance existe et que `app_data_dir` est résolu ; (c) le daemon est **figé** par principe
depuis `feature-app-analytics.md` (D3). Le daemon et le tray vivent et meurent ensemble (sidecar,
`AppState::daemon_child`), donc rien n'est perdu en disponibilité.

**Écarté** : un second abonné MQTT dédié (duplication du transport pour une donnée déjà reçue) ;
et l'écriture depuis le daemon (romprait son gel et dupliquerait la résolution du répertoire).

### D4 — Les rollups sont **recalculés**, jamais incrémentés

**Retenu** : un jour révolu est **figé une fois** ; le jour en cours est **recalculé** à chaque
génération et écrase sa ligne précédente.

**Pourquoi** : un compteur incrémenté dérive dès qu'un scan est manqué ou rejoué, et l'on ne peut
plus jamais le réconcilier avec la source. Un recalcul est idempotent et vérifiable. C'est la même
propriété que revendique le daemon (« recalcul depuis le disque, pas d'incrément mémoire »).

**Corollaire à respecter** : tant qu'un jour est encore présent dans les transcripts, **la source
fait foi** ; le rollup n'est qu'un cache. Ce n'est qu'une fois le jour purgé que le rollup devient
la seule vérité. La lecture doit donc préférer la source quand elle est disponible, et ne se
rabattre sur le rollup que pour les jours disparus.

> **Micro-choix tranché** : les rollups portent les **deux grandeurs nommées de L0** (« Travail »
> et « Volume total »), plus la ventilation par modèle **dès que L2 l'introduit**. Prévoir le champ
> `model` dès maintenant, quitte à le laisser vide tant que L2 n'est pas livré — **une purge est
> irréversible** : ce qu'on n'aura pas capturé ne sera jamais rattrapable.

### D5 — Les commandes de lecture existent, mais rien ne les affiche encore

**Retenu** : exposer `get_quota_history()` et `get_daily_rollups()` (lecture seule, séries triées,
fichier absent → série vide), **sans les brancher à une visualisation**.

**Pourquoi** : le lot est livrable et testable sans interface, et la séquence l'exige (voir l'encart
de priorité). Les consommateurs viendront avec L4 (non engagé). Exposer les commandes maintenant
permet de **vérifier la collecte** sans attendre.

---

## Étapes d'implémentation

1. **Module `quota_history.rs`** (`src-tauri/src/`) : décalquer `memory.rs` — types de ligne à clés
   courtes, `append_sample`, `read_history` (tri + lignes corrompues ignorées), `compact`
   (`.tmp` + `rename` atomique), constantes de cadence et de rétention **nommées**.
2. **Sampler de quota** (D3) : thread détaché lancé au `setup`, qui toutes les 5 min lit
   `AppState::snapshot()` et appende **les seules séries ayant changé** (ou dont le dernier point a
   plus d'une heure). Ne panique jamais ; toute erreur d'I/O est loggée et la boucle continue.
3. **Compaction périodique** : même patron que `memory.rs` (toutes les N écritures + une au
   démarrage), rétention 90 jours.
4. **Module `rollups.rs`** : génération `(jour, projet, provider, agent, model, travail,
   volume_total)` à partir des scans `iatc-core` **post-L0** ; le jour en cours écrase sa ligne,
   les jours révolus sont conservés tels quels (D4).
5. **Déclenchement des rollups** : à l'ouverture de la vue analytics (déjà un point de
   rafraîchissement existant) **et** une fois par jour depuis le thread de fond. Pas de polling
   supplémentaire.
6. **Commandes `get_quota_history` / `get_daily_rollups`** (D5) + enregistrement dans
   `generate_handler!`.
7. **État partagé** : ajouter les chemins résolus à `AppState` (même patron que `MemoryLog`), avec
   leur `Mutex` sérialisant thread de fond et commandes.
8. **Tests** sur dossier temporaire : round-trip, tri, ligne corrompue ignorée, compaction qui
   retire les points hors fenêtre, écriture conditionnelle (valeur inchangée → pas de point ; plus
   d'une heure → point quand même), idempotence du recalcul de rollup.

## Fichiers concernés

- `src-tauri/src/quota_history.rs` — **nouveau**, décalque de `memory.rs`.
- `src-tauri/src/rollups.rs` — **nouveau**, génération et lecture des rollups quotidiens.
- `src-tauri/src/state.rs` — chemins des deux nouveaux journaux dans `AppState`.
- `src-tauri/src/lib.rs` — lancement des threads au `setup`, enregistrement des deux commandes.
- `src/types.ts` — types `QuotaSample`, `DailyRollup` (préparation des consommateurs).
- Tests : `#[cfg(test)]` dans les deux nouveaux modules.

## Comportement attendu

- [ ] Après 30 minutes de fonctionnement, `quota-history.jsonl` contient **au moins un point par
      réservoir connu**, et les points portent `used_pct`, `remaining_pct`, `resets_at`,
      `confidence`.
- [ ] Quota **stable** pendant une heure → **un seul point** par série sur la période (écriture
      conditionnelle), **puis un point horaire forcé**.
- [ ] Quota qui **change** entre deux échantillons → un nouveau point dans les 5 minutes.
- [ ] L'historique **survit à la fermeture de la fenêtre et au redémarrage du tray** (même
      propriété que la courbe RAM).
- [ ] Un point de plus de 90 jours est retiré par la compaction ; le fichier ne croît pas
      indéfiniment.
- [ ] `daily-rollups.jsonl` contient une ligne par `(jour, projet, provider, agent)`, et un
      **recalcul deux fois de suite produit un fichier identique** (idempotence, D4).
- [ ] Les totaux d'un jour dans le rollup **coïncident avec le scan direct** de ce jour
      (réconciliation source / cache tant que la source existe).
- [ ] `get_quota_history()` et `get_daily_rollups()` renvoient des séries triées ; **fichier absent
      → série vide, jamais d'erreur**.
- [ ] Aucune visualisation n'a changé — ce lot n'affiche rien (D5).
- [ ] Le tray ne panique jamais : un répertoire non inscriptible produit un log, pas un crash.

## Vérification

- [ ] `cargo check` / typecheck front OK
- [ ] `cargo clippy` + lint front OK
- [ ] `cargo test` vert (round-trip, tri, corruption, compaction, écriture conditionnelle,
      idempotence du rollup)
- [ ] `bash scripts/quality-report.sh` OK
- [ ] Testé dans l'app réelle : laisser tourner ≥ 1 h, inspecter les deux fichiers sous
      `app_data_dir`, redémarrer le tray et vérifier la reprise sans perte

## Hors scope

- **Toute visualisation** des deux séries (courbe d'épuisement, projection, comparaison de
  comptes) → lot **L4, non engagé**.
- **L'historisation des 176 codes de conso MQTT** : doublon appauvri des logs (sans rétroactivité,
  sans modèle, sans jour). **Seul le quota est historisé.**
- **Toute modification du daemon, du contrat MQTT ou des topics.**
- **Le relèvement de `cleanupPeriodDays`** : réglage personnel du décideur, pas une décision de ce
  projet (D2).
- **Une base de données** (SQLite ou autre) : le JSONL borné suffit, il est déjà éprouvé ici.
- **La synchronisation ou la sauvegarde distante** de ces fichiers.

## Sources

- Proposition et arbitrage : `specs/instructions/proposition-analytics-riche.md` (§ 5 option C et D,
  § 4.5 absence d'identifiant de compte, § 4.6 purge et volumes).
- Cadrage d'origine et clause de réouverture : `specs/instructions/feature-app-analytics.md` (D2).
- Patron de persistance : `specs/instructions/feature-detail-memory-monitor.md` et
  `src-tauri/src/memory.rs`.
- Purge des transcripts Claude Code (`cleanupPeriodDays`, défaut 30 j, `subagents/` compris) :
  [Data usage](https://code.claude.com/docs/en/data-usage),
  [anthropics/claude-code #62476](https://github.com/anthropics/claude-code/issues/62476),
  [Cleanup & Retention](https://brewpirate.github.io/claude-code-docs/sessions/cleanup-retention/).
