# Instruction : transport MQTT sans perte (fin des jauges vides) + publication différentielle

> Cadrée par Gandalf (P1). Consommée par Gimli comme instruction de travail.
> Arbitrage du décideur : **option C = A (transport) puis B (dedup)**, point 9 tranché ci-dessous.

---

## Contexte

**Symptôme utilisateur** : la jauge **5h** du compte `claude/max` ne s'affiche plus dans l'icône du
tray — piste pointillée vide — alors que la donnée source est bonne.

**Chaîne du défaut, mesurée** :

1. La source est saine : `~/.iakatokencounter/quota/claude.max.json` est frais et porte
   `five_hour: {used_percentage: 1.0, resets_at: <futur>}` ⇒ `merge_one` produit
   `confidence: official`, `remaining_pct: 99`.
2. L'état **publié sur le broker** est un patchwork : sur la MÊME fenêtre 5h de `claude/max`,
   `remaining_pct = 99.0` (quelques secondes) mais `confidence = "none"` (figé depuis 1 h 15) et
   `resets_at = null` (figé). Or ces trois codes sont produits **ensemble** par le même `Reservoir`
   (`iakatc-core/src/quota/merge.rs`) et émis dans la même boucle
   (`iakatc-core/src/publish/contract.rs:87`). Sur ~100 topics retained échantillonnés, **28
   seulement** portaient le `t` du dernier tick ; certains dataient de **13 jours**.
3. **Cause racine prouvée par capture** : daemon lancé contre un faux broker qui journalise chaque
   PUBLISH → daemon : `tick … — 227 codes publies (broker connecte)` ; broker : **65 PUBLISH
   capturés**. **162 messages perdus sur 227 (71 %) en une salve.** 65 = capacité du channel (64)
   + 1 en vol. Les 65 rescapés sont **tous** de la famille `all/projets/agents/…`, dans l'ordre
   alphabétique du `BTreeMap`, coupés net à `iakarpgassets` (lettre « i »).
4. Les codes de quota sont émis **après** toute la conso (`contract.rs:193-204` :
   `conso_project_agent` → `conso_provider_agent` → `quota` → `limits` → `meta`) : ils tombent
   **systématiquement** dans la part jetée. Ils n'atteignent le broker que par le resync aléatoire
   sur `ConnAck` (itération sur un `HashMap`) — d'où le patchwork d'âges.
5. **Effet de bord visuel** : `src-tauri/src/icon.rs:89` — `Some("none") => Fill::Unknown` ⇒ piste
   pointillée, jamais de remplissage. Un `confidence` figé à `"none"` suffit à effacer la barre,
   même avec `remaining_pct = 99`.

**Volume par tick (arithmétique vérifiée, elle boucle à l'unité)** :

| famille | calcul | codes |
|---|---|---|
| conso projet × agent | 44 projets × 1 agent × 4 codes | **176 (77 %)** |
| quota | 5 réservoirs × 7 codes | 35 (15 %) |
| conso IA × agent | 2 providers × 4 codes | 8 |
| limits | 2 comptes × 2 plafonds | 4 |
| meta daemon | 4 codes | 4 |
| **total** | | **227** |

**81 % du volume n'a aucun abonné** : le tray ne s'abonne qu'à `{root}/all/ia/+/+/quota/#` et
`{root}/meta/daemon/#` (`src-tauri/src/mqtt_sub.rs:33-34`). Les 184 messages de conso ne sont écoutés
par personne — et ils passent **devant** le quota dans la file, le poussant dehors. L'app analytics
n'en dépend pas : `src-tauri/src/history.rs` produit déjà les tokens par jour et par projet en
scannant directement les fichiers.

## Ce qui existe

| Élément | Où | État |
|---|---|---|
| Publication d'un code | `iakatc-daemon/src/mqtt.rs:84-95` | **Fautif** : `let _ = try_publish(...)` — l'échec est jeté sans log |
| Capacité du channel | `iakatc-daemon/src/mqtt.rs:34` | **Fautif** : `Client::new(opts, 64)` < lot de 227 |
| Resync sur `ConnAck` | `iakatc-daemon/src/mqtt.rs:47-62` | **Fautif** : même `let _ = try_publish`, et itération sur `HashMap` (ordre aléatoire) |
| État mémoire retained | `iakatc-daemon/src/mqtt.rs:20,36` | Présent (`HashMap<topic, payload>`) — base de la dédup de B |
| Boucle de tick | `iakatc-daemon/src/main.rs:114-122` | Publie les 227 messages en rafale et journalise `messages.len()` — **un compteur d'émis, pas de publiés** |
| Ordre d'émission | `iakatc-core/src/publish/contract.rs:193-204` | Conso d'abord, quota ensuite |
| Rendu de l'incertitude | `src-tauri/src/icon.rs:85-95` | Correct — c'est la donnée qui ment, pas le rendu. **Ne pas toucher** |
| Dédup côté tray | `src-tauri/src/state.rs:58-73` | Déjà en place (`set_code` retourne « a changé ») |
| Broker in-process pour test | `iakahub/src/broker.rs` (`broker::start(port)`), exemple `iakahub/tests/broker_roundtrip.rs` | Réutilisable comme harnais d'intégration |
| Cible de test du daemon | `iakatc-daemon/Cargo.toml` | **Bin seul** : pas de `[lib]` ⇒ `mqtt.rs` non testable en intégration aujourd'hui |

## Décision

### A — Réparer le transport (à livrer en premier, seul responsable du symptôme)

Remplacer le fire-and-forget par une publication **qui ne perd rien**, **sans casser la résilience
hors-ligne** documentée en tête de `mqtt.rs` (broker injoignable ⇒ le daemon mesure quand même, ne
se fige jamais, ne panique jamais).

**Faits vérifiés qui contraignent la solution** (rumqttc 0.24) :

- L'erreur de file pleine est **`ClientError::TryRequest`** (et non `TryPublishError::Full` cité au
  diagnostic) ; `ClientError::Request` signale un channel **déconnecté**. Les deux doivent être
  distinguées : la première se retente, la seconde non.
- `Client::publish()` (bloquant) **bloque tant que le channel est plein**, et le channel n'est
  drainé **que** par `Connection::iter()`. ⇒ **interdiction absolue** de faire une publication
  retentée **depuis le thread d'event-loop** (`mqtt.rs:44-72`) : ce serait un auto-blocage — le
  thread qui doit drainer est celui qui attend. C'est précisément le piège du resync `ConnAck`.
- Corollaire : depuis le **thread de tick**, une publication retentée est sûre (l'event-loop tourne
  à côté et draine), à condition d'être **bornée dans le temps** (le broker peut être mort sans que
  `is_connected()` l'ait encore vu).

**Approche retenue** :

1. **Capacité** : `Client::new(opts, 1024)` — largement au-dessus du lot courant (227) et de sa
   croissance. Coût mémoire négligeable (~200 Ko au pire). La capacité n'est plus un critère de
   correction (le retry l'est), seulement du confort.
2. **`publish()` devient fiable et honnête** : `try_publish` ; sur `Err(ClientError::TryRequest)`,
   retenter avec une petite attente (ordre de grandeur : 20 ms, ≤ 50 tentatives ⇒ ~1 s max par
   message) **tant que `is_connected()`**. Échec définitif ⇒ **journaliser** (jamais avaler).
   Hors-ligne (`!is_connected()`) ⇒ **ne pas boucler** : mémoriser l'état et rendre la main
   immédiatement, le resync s'en chargera.
3. **Budget global par lot** : un tick ne doit **jamais** dépasser ~5 s cumulés de retry. Au-delà,
   on arrête de retenter pour ce tick, on journalise le **nombre de topics non publiés**, et on
   laisse l'état mémoire + le resync faire le rattrapage. Garantie testable : *aucun tick ne peut
   se figer, broker mort ou pas*.
4. **Invariant d'état mémoire** : le `HashMap` d'état ne doit refléter que ce qui a été
   **effectivement publié**… **mais** la dernière valeur connue doit être publiable au tick suivant.
   Tenir donc deux informations par topic : la **dernière valeur à publier** (toujours mise à jour)
   et le fait qu'elle ait été **confirmée envoyée**. Un topic non envoyé doit être **réémis au tick
   suivant même si sa valeur n'a pas changé** (c'est ce qui rend B sûr).
5. **Resync `ConnAck`** : ordre **déterministe** (`BTreeMap` ou `sort` sur le topic) et
   **délégation à un thread court** dédié (jamais dans l'event-loop, cf. contrainte ci-dessus),
   protégé par un drapeau « resync en cours » pour ne pas empiler des threads si le lien bat de
   l'aile. Le resync publie **tout** l'état, sans dédup.
6. **Journal de tick honnête** : `main.rs:117-122` doit distinguer **émis** / **publiés** /
   **perdus** (aujourd'hui il annonce 227 alors que 65 passent — c'est ce mensonge qui a masqué le
   bug un mois).

### B — Publication différentielle (ensuite, dans le même lot)

Ne republier un code que si **sa valeur `v` a changé**.

- **La dédup porte sur `v`, jamais sur le payload complet** : le payload contient `t` (epoch du
  tick), qui change à chaque fois — une dédup naïve sur la chaîne ne dédupliquerait **rien**.
- **Sûr pour les consommateurs** : le broker garde le retained (donc la dernière valeur reste
  servie aux nouveaux abonnés) ; le tray ne se sert de `t` que pour marquer la **présence** d'une
  fenêtre (`src-tauri/src/state.rs:71`, `src-tauri/src/icon.rs:113-116`), pas pour périmer une
  valeur. Aucun rendu ne dépend de la fraîcheur de `t`.
- **Ne jamais dédupliquer le resync `ConnAck`** : un broker qui redémarre perd son retained ; le
  resync est la seule chose qui le repeuple.
- **Filet de sécurité** : **resync complet périodique** (tous les 10 ticks ≈ 10 min) même connecté,
  pour borner à 10 min toute divergence silencieuse entre l'état du daemon et le retained du
  broker. Constante nommée, pas de configuration nouvelle.
- **Effet attendu** : après le premier tick, le lot tombe des 227 codes à quelques unités (conso
  des projets touchés + `used_pct`/`used_tokens` qui bougent + `meta/daemon/last_tick_at`).

### Point 9 — défaut de broker du daemon : **tranché, inclus, version minimale**

Constat vérifié : le daemon par défaut sur `192.168.2.11` (`iakatc-daemon/src/config.rs:10`), le
tray sur `127.0.0.1` (`src-tauri/src/config.rs:11`). Ils ne coïncident que parce que **iakahub**
injecte `IAKATC_MQTT_HOST=127.0.0.1` à son enfant (`iakahub/src/supervisor.rs:50`). Lancé à la main
— exactement le geste de la procédure de vérification ci-dessous — **le daemon part sur le LAN et
publie dans un broker tiers**.

**Décision : aligner le défaut du daemon sur `127.0.0.1`.** Justification : l'architecture retenue
est le backbone **iakahub local** (broker `rumqttd` sur `127.0.0.1`, `iakahub/src/broker.rs`) ; le
défaut LAN est un vestige antérieur à iakahub. Le LAN reste atteignable par `IAKATC_MQTT_HOST`, le
contrat n'impose rien d'autre. Corollaire obligatoire : mettre à jour
`specs/contrat-mqtt-conso.md` § 6 (ligne « Hôte | `192.168.2.11` ») qui documente encore l'ancien
défaut — sinon le contrat ment. **C'est le seul point de cette instruction qui change un
comportement documenté ; le valider, c'est valider ce changement.**

## Périmètre

**Inclus** — A, puis B, puis le point 9 ; les tests correspondants ; la mise à jour de la ligne
concernée du contrat MQTT ; l'ajout d'une cible `[lib]` au crate `iakatc-daemon` (strict nécessaire
pour rendre `mqtt.rs` testable en intégration).

**Exclu — explicitement hors de ce lot** :

- Écran « coût par projet » dans l'analytics (suite possible, **non spécifiée ici**).
- Supprimer ou réduire les 184 codes de conso sans abonné : B les fait s'effondrer d'eux-mêmes ;
  on ne coupe aucune famille de topics dans ce lot (le contrat MQTT § 2 les décrit).
- Réordonner `tick_messages` pour émettre le quota avant la conso (`contract.rs:193-204`) : inutile
  une fois A en place — *tant qu'on y est* interdit.
- Retirer l'axe `agent` du contrat, bien qu'il soit plat aujourd'hui (`Agent::{Coordinator,
  Subagent}`, `iakatc-core/src/measure/mod.rs:32` ; zéro tour `isSidechain:true` sur 626 fichiers) :
  décision de contrat, à cadrer à part.
- Tout changement de `src-tauri/src/icon.rs` : le rendu est conforme à la spec, il ne ment pas.
- Toute modification du broker iakabox ou de iakaboxlogs.

## Étapes d'implémentation

**Phase A — transport (livrable indépendant, commit(s) dédié(s))**

1. Ajouter `[lib]` à `iakatc-daemon` (`src/lib.rs` exposant `pub mod config; pub mod mqtt;`),
   `main.rs` consommant la lib. Aucun changement de comportement.
2. `mqtt.rs` : porter la capacité à `1024` et extraire un helper de publication retentée
   (bornes : ~20 ms × 50 tentatives max par message, budget global du lot ~5 s, arrêt immédiat si
   `!is_connected()`, distinction `ClientError::TryRequest` / `ClientError::Request`), avec
   journalisation de tout abandon.
3. `mqtt.rs` : tenir par topic la dernière valeur **à publier** + son état **envoyé/non envoyé** ;
   un topic non envoyé est réémis au tick suivant.
4. `mqtt.rs` : resync `ConnAck` en **ordre déterministe**, exécuté dans un **thread court dédié**
   (jamais dans l'event-loop), avec drapeau anti-empilement.
5. `main.rs` : journaliser `émis / publiés / perdus` au lieu de `messages.len()`.
6. Test d'intégration `iakatc-daemon/tests/` (dev-dependency `iakahub` en chemin + `rumqttc`) :
   broker in-process sur port libre (cf. `iakahub/tests/broker_roundtrip.rs`), publier un lot de
   **300 topics distincts** (> capacité historique et > lot réel), un abonné `#` vérifie que les
   **300 arrivent** — aucun manquant.
7. Test hors-ligne : publisher pointé sur un port **fermé**, publier 300 messages, vérifier que
   l'appel **rend la main en quelques secondes** (borne du budget) et **ne panique pas**.

**Phase B — publication différentielle (commit distinct, après A verte)**

8. `mqtt.rs` : sauter la publication d'un topic dont la valeur `v` est inchangée **et** dont le
   dernier envoi a été confirmé.
9. Resync `ConnAck` **sans dédup** + **resync complet périodique** (constante : tous les 10 ticks).
10. Tests unitaires de dédup : (a) même `v`, `t` différent ⇒ **pas** de republication ;
    (b) `v` différent ⇒ republication ; (c) un topic dont l'envoi a échoué est réémis au tick
    suivant à `v` inchangé ; (d) le resync republie **tout**.

**Point 9 (commit distinct, trivial)**

11. `iakatc-daemon/src/config.rs:10` : `DEFAULT_HOST = "127.0.0.1"` + test du défaut ; mettre à jour
    la ligne « Hôte » du § 6 de `specs/contrat-mqtt-conso.md`.

## Fichiers concernés

- `iakatc-daemon/src/mqtt.rs` — cœur du correctif (A et B).
- `iakatc-daemon/src/main.rs` — journal de tick honnête ; passage par la lib.
- `iakatc-daemon/src/lib.rs` — **nouveau** : expose `config` et `mqtt` pour les tests d'intégration.
- `iakatc-daemon/Cargo.toml` — cible `[lib]` + dev-dependencies (`iakahub` en chemin, `rumqttc`).
- `iakatc-daemon/tests/mqtt_no_loss.rs` — **nouveau** : lot > capacité, zéro perte ; hors-ligne borné.
- `iakatc-daemon/src/config.rs` — défaut d'hôte (point 9).
- `specs/contrat-mqtt-conso.md` — § 6, ligne « Hôte » (point 9).
- **Non modifiés** : `iakatc-core/src/publish/contract.rs`, `src-tauri/src/icon.rs`,
  `src-tauri/src/mqtt_sub.rs`.

## Risques

- **Auto-blocage du thread d'event-loop** (le plus grave) : toute publication retentée exécutée
  dans `connection.iter()` bloque le drainage du channel ⇒ deadlock garanti. *Mitigation* : resync
  délégué à un thread court ; à vérifier en revue de code, pas seulement en test.
- **Régression de résilience hors-ligne** : un `publish()` bloquant sans borne fige le tick. Le
  daemon doit continuer de mesurer broker mort. *Mitigation* : budget de retry + test hors-ligne
  dédié (étape 7), qui est un critère d'acceptation à part entière.
- **Dédup qui masque une désynchronisation** : broker vidé de son retained sans coupure TCP ⇒ des
  topics jamais réémis. *Mitigation* : resync périodique (10 min) + réémission forcée des envois
  non confirmés.
- **Flapping de connexion** : ConnAck répétés ⇒ threads de resync empilés. *Mitigation* : drapeau
  « resync en cours ».
- **Multiplication des threads / tests flaky sur port** : réutiliser le motif `free_port()` éprouvé
  de `iakahub/tests/broker_roundtrip.rs`, et borner toutes les attentes par une deadline.

## Comportement attendu

- Le daemon publie **l'intégralité** des codes d'un tick tant que le broker est joignable.
- Sur la même fenêtre 5h, `remaining_pct`, `confidence` et `resets_at` portent **le même `t`** — le
  patchwork disparaît.
- Les **deux barres** (5h et 7j) de `claude/max` réapparaissent remplies dans l'icône du tray.
- Broker absent : le daemon continue de mesurer, journalise, ne se fige pas, ne panique pas, et
  republie tout à la reconnexion.
- Après B, le nombre de messages réellement publiés par tick s'effondre en régime stable.

## Vérification

### Critères d'acceptation

- [ ] **A1** — Test d'intégration : lot de **300** topics distincts via `MqttPublisher` sur broker
      in-process ⇒ un abonné `#` reçoit **les 300** (aucun topic manquant). Le test échoue sur le
      code actuel.
- [ ] **A2** — Test hors-ligne : publisher sur port fermé, 300 messages ⇒ retour en **< 10 s**,
      aucun panic, l'état est conservé.
- [ ] **A3** — Test de resync : après coupure/reprise du broker, **tous** les topics de l'état sont
      republiés, et dans un **ordre déterministe**.
- [ ] **A4** — Aucune publication retentée n'est exécutée dans le thread `connection.iter()`
      (vérifiable par lecture du diff — critère de revue explicite).
- [ ] **A5 (reproduction de la mesure du diagnostic)** — daemon réel lancé contre un broker de
      capture qui journalise chaque PUBLISH : le **nombre de PUBLISH reçus est égal au nombre de
      codes annoncés par le tick** (aujourd'hui : 227 annoncés / 65 reçus). Consigner les deux
      chiffres avant/après dans le message de commit ou le rapport qualité.
- [ ] **A6** — Le log de tick affiche `émis / publiés / perdus` et `perdus = 0` broker joignable.
- [ ] **B1** — Valeur `v` inchangée et envoi confirmé ⇒ **aucune** republication (malgré `t` qui
      change) ; valeur changée ⇒ republication.
- [ ] **B2** — Envoi non confirmé ⇒ réémission au tick suivant même à `v` inchangé.
- [ ] **B3** — Resync `ConnAck` et resync périodique publient **tout l'état**, dédup ignorée ; un
      abonné qui se connecte après plusieurs ticks dédupliqués reçoit **toutes** les valeurs
      courantes via le retained.
- [ ] **B4** — En régime stable (2ᵉ tick et suivants), le nombre de messages publiés par tick est
      **très inférieur** à 227 (chiffre relevé et consigné).
- [ ] **9** — Défaut `IAKATC_MQTT_HOST` du daemon = `127.0.0.1` (test) ; § 6 du contrat à jour.
- [ ] **Recette utilisateur** — dans l'app réelle (tray lancé normalement), les **deux barres** de
      `claude/max` sont pleines et cohérentes avec
      `~/.iakatokencounter/quota/claude.max.json` ; plus aucune piste pointillée sur une fenêtre
      dont la donnée source est fraîche.
- [ ] `cargo fmt` / `cargo clippy` propres, `cargo test` vert sur tout le workspace
      (`bash scripts/quality-report.sh`).

## Hors scope

- Écran « coût par projet » (analytics) — suite possible.
- Suppression/réduction des familles de topics sans abonné (conso projet × agent, IA × agent).
- Réordonnancement de `tick_messages`.
- Retrait de l'axe `agent` du contrat.
- Toute modification du rendu d'icône (`src-tauri/src/icon.rs`).

---

## Sources externes consultées (rumqttc 0.24)

- `ClientError` a deux variantes : `Request` (channel déconnecté) et `TryRequest` (channel plein,
  envoi non bloquant refusé) — https://docs.rs/rumqttc/0.24.0/rumqttc/enum.ClientError.html
- `Client::new(options, cap)` : `cap` = capacité du channel borné ; `publish`/`try_publish`
  renvoient `Result<(), ClientError>` — https://docs.rs/rumqttc/0.24.0/rumqttc/struct.Client.html
- Le channel n'est drainé que par le polling de l'event-loop (`Connection::iter()` côté client
  sync) ; publier depuis le thread qui pilote l'event-loop avec un channel plein s'auto-bloque —
  https://rumqtt.bytebeam.io/docs/rumqttc/Developer%20Guide/rumqttc%20internals/
