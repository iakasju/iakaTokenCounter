# Instruction : iakahub v0 (broker MQTT local embarqué + orchestration)

> Rédigé par Gandalf (P1 — cadrage). Consommé par Gimli comme instruction de travail.
> Backbone **local et autonome** : supprime toute dépendance à un broker externe authentifié.
> **Interfaces voisines figées** : contrat `specs/contrat-mqtt-conso.md`, daemon
> `specs/instructions/feature-collecteur-logs.md`, tray `specs/instructions/feature-tray-jauges.md`.

---

## Contexte

La **recette live** (journal `PROJET.md`, 2026-07-08) a montré que le broker Mosquitto d'iakabox
(`192.168.2.11:1883`) exige un login (`allow_anonymous false`) et que **le mot de passe est perdu /
irrécupérable** → le daemon ne peut pas publier, les jauges du tray restent **vides**. Décision
décideur : **ne plus dépendre d'un broker externe**. On introduit **iakahub**, un **broker MQTT
LOCAL embarqué** (`127.0.0.1`, anonyme), qui rend le poste **standalone total**.

**3 arbitrages verrouillés (décideur, 2026-07-08)** :
1. **iakahub naît ici**, dans iakaTokenCounter (vocation portefeuille ultérieure — ressort d'Odin).
2. **iakahub = broker + orchestration SEUL.** La **mesure reste séparée** : le measure daemon
   `iakatc-daemon` est une brique distincte qui **se connecte** à iakahub. On **ne touche pas** à sa
   logique de mesure/publication (uniquement l'**env** qu'on lui passe).
3. **Local d'abord.** Un **bridge vers Mosquitto iakabox** est une **option ultérieure**, hors MVP.

Cette instruction ferme le périmètre d'**iakahub v0** : démarrer un broker in-process local, **spawner
et superviser** le daemon en lui injectant l'env broker, et **rebrancher la GUI** dessus (édits
minimes des fichiers tray existants).

### Faits vérifiés (veille Gandalf, sources en bas)

- **`rumqttd`** (le **broker** pendant de `rumqttc`, même écurie bytebeam) s'**embarque en
  bibliothèque** : on construit une **`rumqttd::Config`** (dérive `serde::Deserialize` → typiquement
  désérialisée depuis un **TOML**), puis `let mut broker = Broker::new(config); broker.start();`.
  **`broker.start()` est BLOQUANT** (ne rend la main que quand tous les serveurs configurés
  s'arrêtent) → il faut l'appeler dans un **thread dédié** (`std::thread::spawn`). Le TOML par défaut
  du dépôt expose un listener **MQTT v4 (3.1.1) sur 1883** et un listener v5 sur 1884 ; les **ports et
  l'adresse de bind sont configurables**. Diagnostics via `tracing-subscriber`.
- **`rumqttc`** (déjà utilisé côté daemon et tray) se connecte en TCP à `127.0.0.1:<port>` ; sur un
  broker **sans auth**, un CONNECT est accepté **avec ou sans** username/password (les creds éventuels
  sont ignorés) → on peut injecter des **creds factices** au daemon sans changer son code.
- **Contrat & daemon** : le daemon lit sa cible broker via **`IAKATC_MQTT_HOST` / `IAKATC_MQTT_PORT`
  / `IAKATC_MQTT_USER` / `IAKATC_MQTT_PASSWORD`** (contrat § 6). Il suffit de **poser ces variables**
  dans l'environnement du process enfant pour le pointer sur iakahub — **zéro modif de code daemon**.
- **Tray (D1)** : la GUI spawne aujourd'hui `iakatc-daemon` en **sidecar** (`bundle.externalBin` +
  `ShellExt::sidecar("iakatc-daemon")`) et son subscriber pointe sur `192.168.2.11`. Ce câblage
  **bascule** sur iakahub (voir D4).

## Ce qui existe

| Élément | Où | État |
|---|---|---|
| Décision iakahub + Cap | `specs/PROJET.md` (§ Cap + journal 2026-07-08) | à jour |
| Contrat MQTT (topics/env broker) | `specs/contrat-mqtt-conso.md` | figé |
| Measure daemon `iakatc-daemon` | crate `iakatc-daemon` (+ `iakatc-core`) | figé ; en cours (Gimli) |
| GUI tray (subscriber + spawn sidecar) | `src-tauri/` (`mqtt_sub.rs`, `lib.rs`, `tauri.conf.json`) | en cours (Gimli) |
| Broker externe Mosquitto iakabox | `192.168.2.11:1883` (auth, mdp perdu) | **abandonné comme cible MVP** |
| Crate `iakahub` | — | **absente (objet de cette instruction)** |

## Décision

### D1 — Nouvelle crate/bin Rust **`iakahub`** embarquant `rumqttd` (broker in-process, local)

**Retenu** : un **binaire Rust `iakahub`** (membre du workspace) qui **démarre un broker `rumqttd`
in-process** dans un **thread dédié** (`broker.start()` est bloquant), écoutant en **`127.0.0.1`**,
**port par défaut `1883`** (configurable via **`IAKATC_MQTT_PORT`**), **listener MQTT v4 unique**,
**anonyme** (aucune auth).

**Pourquoi rumqttd** : c'est le broker de la même écurie que `rumqttc` (déjà dans la stack) →
cohérence, embarquable en bibliothèque, pas de dépendance à un Mosquitto externe. Bind sur
**`127.0.0.1`** (et non `0.0.0.0`) : le broker n'est **pas exposé au réseau** — standalone et sûr par
défaut.

**Écarté** : *garder Mosquitto iakabox* (mdp perdu, dépendance réseau) ; *broker sur `0.0.0.0`*
(exposition inutile au MVP) ; *écrire un broker maison* (rumqttd existe et est éprouvé).

> **Micro-choix tranché — construction de la `Config`** : embarquer un **gabarit TOML** dans le
> binaire (`include_str!`), y **substituer le port** (et le bind `127.0.0.1`), puis
> `toml`→`rumqttd::Config`. Plus robuste que de construire la struct champ par champ (dont les noms
> varient selon la version de `rumqttd`). Gimli **épingle une version de `rumqttd`** et cale le TOML
> dessus. Un seul listener **v4** (pas de v5, pas de WebSocket — hors scope).

### D2 — Orchestration : **iakahub spawne et supervise `iakatc-daemon`** (un seul sidecar)

**Retenu** : au démarrage, iakahub **(1) lance le broker** (thread), **(2) attend qu'il soit à
l'écoute**, puis **(3) spawne `iakatc-daemon`** en process enfant (`std::process::Command`) en lui
**injectant l'environnement broker** :

```
IAKATC_MQTT_HOST=127.0.0.1
IAKATC_MQTT_PORT=<port iakahub>
IAKATC_MQTT_USER=iakahub          # creds factices : broker anonyme → ignorés
IAKATC_MQTT_PASSWORD=local
```

**Pourquoi iakahub-orchestre (et pas deux sidecars indépendants)** : la GUI n'a plus qu'**UN**
compagnon à lancer (iakahub), qui garantit **l'ordre** (broker prêt **avant** le daemon) et une
**vie liée** (fermer iakahub ferme le daemon). Les creds factices satisfont l'exigence « creds
obligatoires » du daemon **sans modifier son code** (fait vérifié : broker anonyme les ignore).

**Supervision** : si l'enfant `iakatc-daemon` **se termine de façon inattendue** pendant qu'iakahub
tourne, iakahub **journalise** et **retente** un redémarrage **borné** (backoff, **≤ 3 tentatives**),
puis **abandonne en journalisant** (pas de boucle de crash). Le broker, lui, **continue** de tourner.

> **Micro-choix tranché** : iakahub est le **parent orchestrateur** ; le daemon n'est **jamais**
> spawné directement par la GUI. Redémarrage enfant **borné à 3** puis arrêt propre.

### D3 — Arrêt propre : pas de daemon orphelin

**Retenu** : iakahub installe un **handler d'arrêt** (Ctrl-C / SIGTERM ; sur Windows, l'événement de
fermeture du process) qui **tue l'enfant `iakatc-daemon`** avant de rendre la main, et applique aussi
un **kill-on-drop** sur le handle enfant. Quand la GUI (parent du sidecar iakahub) se ferme, Tauri
termine iakahub → iakahub termine le daemon → **aucun orphelin**.

**Pourquoi** : un daemon orphelin continuerait à publier/mesurer sans broker parent → fuite de
process cross-OS. L'arrêt en cascade est explicite.

### D4 — Rebranchement GUI : la GUI spawne **iakahub** (édits minimes des fichiers tray)

> **Ne réécrit PAS l'instruction tray** ; en **met à jour l'intention de D1**. Édits **minimes**, à
> faire par **Gimli** (pas par Gandalf). La **logique d'agrégation du tray reste inchangée** — seuls
> changent le **binaire spawné** et le **défaut d'hôte**.

Fichiers existants à toucher (édits ciblés) :

| Fichier | Édit minime |
|---|---|
| `src-tauri/tauri.conf.json` | `bundle.externalBin` : remplacer `iakatc-daemon` par **`iakahub`** (c'est iakahub qu'on embarque et lance ; il porte le daemon). |
| `src-tauri/src/lib.rs` (`setup`) | `app.shell().sidecar("iakatc-daemon")` → **`sidecar("iakahub")`**. Le flag `IAKATC_SPAWN_DAEMON` devient l'interrupteur de spawn d'**iakahub** (sémantique inchangée : `false` = ne rien spawner, cas d'un iakahub géré par le système). |
| `src-tauri/src/mqtt_sub.rs` | **défaut d'hôte** : `192.168.2.11` → **`127.0.0.1`** (toujours surchargeable par `IAKATC_MQTT_HOST`). Creds : accepter l'absence/placeholder (broker local anonyme). |
| `README.md` (app) | mentionner iakahub (broker local) au lieu du broker distant ; défaut `127.0.0.1`. |

**Empaquetage sidecar** : `iakahub` doit être compilé avec le **suffixe `-$TARGET_TRIPLE`** et
**`iakatc-daemon` doit être disponible** pour qu'iakahub le spawne — **micro-choix tranché** :
au MVP, `iakahub` **localise `iakatc-daemon` à côté de son propre exécutable** (même dossier), et les
**deux binaires sont placés dans `externalBin`** (ou co-embarqués). Le détail d'empaquetage
multi-binaires est un point d'implémentation Gimli ; l'intention est : **iakahub trouve le daemon
près de lui**, sinon il journalise « daemon introuvable » et laisse le broker tourner.

### D5 — Robustesse au démarrage : échec **propre**, jamais de crash silencieux

- **Port déjà pris** (`127.0.0.1:<port>` occupé) ou **broker qui ne démarre pas** : iakahub
  **journalise une erreur explicite** (« port 1883 occupé », etc.) et **sort en code ≠ 0** (échec
  visible). **Pas d'auto-incrément de port au MVP** (comportement déterministe ; l'utilisateur peut
  fixer `IAKATC_MQTT_PORT`). Côté GUI, la fin du sidecar iakahub déclenche le **bandeau « backbone
  indisponible »** (réutilise le mécanisme « daemon indisponible » du tray).
- **Broker OK mais daemon KO** : broker maintenu, supervision D2 (retente bornée), bandeau si abandon.
- Aucune de ces situations ne **panique** ni ne laisse un état ambigu : log clair + code de sortie.

> **Micro-choix tranché** : **fail-fast** sur port occupé (pas de fallback de port), configurable par
> env. Simplicité et déterminisme MVP.

## Étapes d'implémentation

1. **Crate `iakahub`** (bin) dans le workspace (`Cargo.toml` racine). Dépendances : `rumqttd`
   (épinglé), `toml`/`serde`, `tracing`/`tracing-subscriber`. **Aucune** dépendance à `iakatc-core`.
2. **Broker in-process** (`iakahub/src/broker.rs`) : gabarit TOML embarqué (`include_str!`),
   substitution `bind=127.0.0.1` + `port=IAKATC_MQTT_PORT` (défaut 1883), listener **v4 seul** ;
   `Broker::new(config)` ; `broker.start()` dans un **thread dédié** ; attente « à l'écoute »
   (probe TCP courte / court délai borné) avant de rendre la main.
3. **Orchestration daemon** (`iakahub/src/supervisor.rs`) : `Command` pour `iakatc-daemon` (localisé
   près de l'exécutable iakahub) + env injecté (D2) ; supervision avec **redémarrage borné (≤ 3)**.
4. **Arrêt propre** (`iakahub/src/shutdown.rs`) : handler Ctrl-C/SIGTERM + kill-on-drop de l'enfant
   (D3).
5. **`main.rs`** : init tracing → broker (thread) → attente écoute → spawn+supervise daemon → attente
   signal d'arrêt → kill enfant → exit. Échecs = log + code ≠ 0 (D5).
6. **Rebranchement GUI** (D4) : **édits minimes** (Gimli) de `tauri.conf.json`, `lib.rs`,
   `mqtt_sub.rs`, `README.md` (externalBin `iakahub`, sidecar `iakahub`, hôte défaut `127.0.0.1`).
7. **Tests** (§ Comportement) : round-trip in-process via `rumqttc` ; logique de supervision.
8. **README `iakahub`** : rôle (broker local + orchestration), env (`IAKATC_MQTT_PORT`), bind
   `127.0.0.1` anonyme, comportement port occupé, arrêt en cascade, renvoi au contrat.

## Fichiers concernés

**Créés (crate `iakahub`)** :
- `iakahub/Cargo.toml` — deps `rumqttd` (épinglé), `toml`, `serde`, `tracing(-subscriber)`.
- `iakahub/src/main.rs` — orchestration de vie (broker → daemon → arrêt).
- `iakahub/src/broker.rs` — config TOML embarquée + démarrage rumqttd (thread).
- `iakahub/src/supervisor.rs` — spawn + env injecté + supervision bornée du daemon.
- `iakahub/src/shutdown.rs` — arrêt propre / kill enfant.
- `iakahub/rumqttd.toml` (gabarit embarqué) — listener v4 `127.0.0.1:{port}`, anonyme.
- `iakahub/README.md` — doc d'usage.
- `Cargo.toml` (workspace) — ajout du membre `iakahub`.

**Touchés (édits minimes, Gimli — pas de changement de logique)** :
- `src-tauri/tauri.conf.json` — `externalBin` → `iakahub` (+ daemin co-embarqué).
- `src-tauri/src/lib.rs` — `sidecar("iakahub")`.
- `src-tauri/src/mqtt_sub.rs` — défaut hôte `127.0.0.1` ; creds locales optionnelles.
- `src-tauri/README.md` — mention iakahub / hôte local.

> **NON touchés** : la logique de **mesure/publication** d'`iakatc-daemon`/`iakatc-core` et la logique
> d'**agrégation** du tray. Le daemon n'est modifié **ni en code ni en config** — seulement l'**env**
> qu'iakahub lui passe.

## Comportement attendu

Critères **observables et testables** :

- **Broker in-process** : au lancement d'`iakahub`, un client `rumqttc` qui se connecte à
  `127.0.0.1:1883` (anonyme) **réussit le CONNECT**, peut **publier un message retained** puis, via un
  **second client** qui s'abonne au même topic, **reçoit immédiatement** la valeur retained
  (round-trip local, **sans broker externe**). *(test d'intégration in-process)*
- **Bout-en-bout orchestré** : au lancement d'`iakahub` (avec `iakatc-daemon` présent à côté), un
  client `rumqttc` abonné à `iakatokencounter/#` **reçoit une valeur retained publiée par le daemon**
  (ex. `meta/daemon/state/current` = `{"v":"up",…}`) — preuve que l'env injecté a bien pointé le
  daemon sur `127.0.0.1`.
- **Port occupé** : si `127.0.0.1:1883` est déjà pris, `iakahub` **journalise l'erreur** et **sort en
  code ≠ 0** (aucun crash silencieux, aucun auto-incrément).
- **Supervision** : si le daemon enfant meurt, `iakahub` **retente** (≤ 3) puis **abandonne en
  journalisant** ; le **broker reste actif** (un client peut toujours se connecter). *(testable au
  niveau logique de la fonction de supervision)*
- **Arrêt en cascade** : à l'arrêt d'`iakahub` (signal), **aucun process `iakatc-daemon` ne survit**
  (pas d'orphelin). *(vérif observable : le PID enfant n'existe plus)*
- **GUI** : après rebranchement (D4), le tray **spawne iakahub** (et non le daemon directement), son
  subscriber pointe sur **`127.0.0.1`**, et les **jauges se remplissent** en local sans aucun accès à
  `192.168.2.11`.
- **`IAKATC_SPAWN_DAEMON=false`** : la GUI **ne spawne pas** iakahub (cas d'un iakahub géré par le
  système) et fonctionne en subscriber pur sur `127.0.0.1`.

## Vérification

- [ ] `cargo check` / clippy OK (workspace, crate `iakahub`)
- [ ] `cargo test` vert : round-trip broker in-process (connect + publish retained + subscribe +
      reçu) ; logique de supervision (retente bornée) ; substitution de port dans la config
- [ ] `iakahub` démarre seul (broker écoute sur `127.0.0.1`, vérifiable par `mosquitto_sub`/`rumqttc`)
- [ ] Édits GUI minimes appliqués (externalBin/sidecar/hôte) — build Tauri OK
- [ ] Testé dans l'app réelle : lancement du tray → iakahub + daemon actifs → **jauges remplies en
      local** ; fermeture du tray → **aucun orphelin** ; port occupé → message clair

## Hors scope

- **Bridge vers le Mosquitto iakabox** (`192.168.2.11`) et toute connexion à un broker distant.
- **Auth / TLS** sur le broker local (anonyme `127.0.0.1` au MVP).
- **Listener WebSocket** (port `9883`-like) et **listener MQTT v5** (v4 seul au MVP).
- **Absorption des logs**, **routage des conversations**, **persistance** (CouchDB ou autre).
- **Toute modification de la logique** de mesure/publication d'`iakatc-daemon`/`iakatc-core** (on ne
  change que l'**env** injecté) et de l'**agrégation** du tray (on ne change que l'hôte par défaut +
  le binaire spawné).
- **Auto-incrément de port** / découverte de port (fail-fast configurable, D5).
- **Empaquetage multi-binaires avancé** (signature, notarisation) au-delà de la convention
  `externalBin` + target-triple.

---

## Sources (veille)

- rumqttd — Embedding in your application (`Config` serde/TOML, `Broker::new`, `broker.start()`
  bloquant → thread, listeners v4 1883 / v5 1884) :
  https://rumqtt.bytebeam.io/docs/rumqttd/Guides/Embedding%20rumqttd%20in%20your%20application/
- rumqttd — crates.io : https://crates.io/crates/rumqttd
- rumqttd `broker.start()` en thread (forum Rust) :
  https://users.rust-lang.org/t/rumqttd-broker-start-with-tokio-spawn/114483
- rumqttc (client, connexion locale + retained) : https://crates.io/crates/rumqttc
- Env broker & topics consommés : `specs/contrat-mqtt-conso.md`
- Cap iakahub + décision 2026-07-08 : `specs/PROJET.md`
