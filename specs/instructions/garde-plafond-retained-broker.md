# Instruction : plafond de rattrapage du retained (broker → abonné) — garde-fou et documentation

> Cadrée par Gandalf (P1). Consommée par Gimli comme instruction de travail.
> Pendant, côté **broker → abonné**, du lot `fix-mqtt-transport-pertes.md` (côté **daemon → broker**).

---

## Verdict de cadrage (à lire en premier)

**Il n'y a pas de défaut à corriger aujourd'hui, et le levier évident n'existe pas.** Ce lot ne
répare rien : il **fige par la mesure** un plafond réel mais non atteint, et **outille** le jour où
il le sera. Trois conclusions, toutes vérifiées dans le code :

1. **Le tray n'est pas exposé.** Ses deux filtres ne couvrent pas les 227 topics retained du broker
   mais **39** (35 quota + 4 meta) — loin des 100. L'hypothèse de départ est infirmée.
2. **Le plafond est réel, dur et silencieux** — mais **il n'est pas celui qu'on croyait** : ce n'est
   pas une fenêtre qui se débloque, c'est une **troncature définitive** d'un **sous-ensemble
   arbitraire** du retained, au moment de l'abonnement.
3. **`max_inflight_count` n'est pas le levier.** La clé du TOML ne pilote **pas** cette fenêtre ;
   la relever serait un **no-op** doublé d'une fausse sécurité. Monter de version non plus : le
   `main` amont porte encore le même code.

---

## Problème

Un abonné qui se (re)connecte à `iakahub` et souscrit à un filtre couvrant plus de **100** topics
retained n'en reçoit que **100** — au hasard — et **ne reçoit jamais les autres** sur cet
abonnement. Le tray reste sous ce seuil aujourd'hui (39), mais la marge est **finie, non
documentée, et franchie sans le moindre signal** : ni erreur, ni log, ni symptôme — juste un état
initial partiel, donc un rendu qui ment. C'est exactement la pathologie du lot A (patchwork d'âges
sur une même fenêtre de quota), déplacée de l'autre côté du broker.

## Ce qui est établi (mesures à ne PAS refaire)

Mesures de Legolas sur le broker `iakahub::broker` réel, deux sondes, réutilisées telles quelles :

| Sonde | Dispositif | Résultat |
|---|---|---|
| **Fan-out live** | abonné connecté et souscrit **avant** l'émission, 300 PUBLISH QoS 1 | **300/300 en 0,38 s**, stable sur 3 répétitions — **aucun plafond** |
| **Rattrapage de backlog** | 300 PUBLISH retained **avant** la connexion de l'abonné, puis connexion + souscription | **100/300 en 10 s**, **plafonné exactement à 100 et durablement bloqué** |

Faits structurants déjà acquis, non re-questionnés ici : `iakahub` **ne persiste pas** le retained
sur disque (tout redémarrage repart d'un broker vide, repeuplé en < 10 s par le daemon) ; le lot A
(transport sans perte) est livré et vert (227 émis / 227 reçus).

## Cause racine (lue dans le code de `rumqttd` 0.19, pas déduite)

`rumqttd-0.19.0/src/router/routing.rs:1455-1467`, dans `forward_device_data`, à la **première**
lecture d'un abonnement :

```rust
if request.forward_retained {
    // NOTE: ideally we want to limit the number of read messages
    // and skip the messages previously read while reading next time.
    // but for now, we just try to read all messages and drop the excess ones
    let mut retained_publishes = datalog.read_retained_messages(&request.filter);
    retained_publishes.truncate(inflight_slots as usize);
    publishes.extend(retained_publishes.into_iter().map(|p| (p, None)));
    inflight_slots -= publishes.len() as u64;
    // we only want to forward retained messages once
    request.forward_retained = false;
}
```

Quatre conséquences, chacune vérifiable à la lecture :

1. **La troncature est définitive.** `forward_retained = false` juste après : l'excédent n'est
   **jamais** relu. Le commentaire amont le dit mot pour mot — *« we just try to read all messages
   and **drop the excess ones** »*. D'où le « durablement bloqué » de la sonde de backlog : ce n'est
   pas un débit bridé, c'est une **perte**.
2. **Le sous-ensemble conservé est arbitraire.** `read_retained_messages`
   (`router/logs.rs:272-307`) itère un `HashMap<Topic, PublishData>` : **l'ordre n'est pas
   spécifié**. Les 100 rescapés sont donc un tirage — un patchwork, pas un préfixe. Sur un quota,
   cela reproduit à l'identique le défaut du lot A : `remaining_pct` présent, `confidence` absent.
3. **`inflight_slots` ne vient pas de la configuration.** Pour QoS ≥ 1 c'est
   `outgoing.free_slots()` = `MAX_INFLIGHT - inflight_buffer.len()`, où
   `MAX_INFLIGHT` est une **constante de compilation valant 100** (`router/iobufs.rs:18`).
   La clé `max_inflight_count` du TOML, elle, est passée à `Network::new(...)` en position
   `max_connection_buffer_len` (`server/broker.rs:498-503` → `link/network.rs:43-58`) : c'est une
   **taille de tampon réseau**, rien d'autre. **Le nom de la clé ment.** Pour QoS 0, et seulement
   pour QoS 0, `inflight_slots = max_outgoing_packet_count` (200) — d'où les deux artefacts
   observés au diagnostic.
4. **C'est par filtre et par connexion.** La troncature s'applique au sous-ensemble retained
   **matchant un filtre**, avec les slots **libres à cet instant** — donc les filtres d'une même
   connexion se **partagent** la fenêtre de 100 s'ils sont servis avant le retour des PUBACK.

Le chemin **live** (`native_readv` avec curseur) est, lui, **repris** à chaque PUBACK
(`router/scheduler.rs:163`) : il est bridé, jamais tronqué. D'où l'asymétrie mesurée — 300/300 en
live, 100/300 en rattrapage.

## Exposition réelle du tray (l'hypothèse de départ, tranchée)

`src-tauri/src/mqtt_sub.rs:33-34` pose **deux** filtres, en QoS 1 :

| Filtre | Topics retained couverts | Détail |
|---|---|---|
| `{root}/all/ia/+/+/quota/#` | **35** | 5 réservoirs × 7 codes (`contract.rs:87-118`) |
| `{root}/meta/daemon/#` | **4** | `state`, `last_tick_at`, `broker_connected`, `version` |
| **total** | **39** | sur 227 retained présents au broker |

Les 188 autres topics (conso projet × agent, conso IA × agent, limits) **ne matchent aucun des deux
filtres** : `all/ia/agents/{provider}/{agent}/conso/…` et `all/ia/{p}/{a}/limits/…` échouent tous
deux sur le segment littéral `quota`. **Le tray est à 39/100 — il n'est pas exposé.**

**Seuil de rupture, chiffré** : le filtre quota croît de **7 topics par réservoir**, soit **14 par
compte** (2 fenêtres). Il franchit 100 au **15ᵉ réservoir**, c'est-à-dire au **8ᵉ compte IA
surveillé**. Aujourd'hui : 5 réservoirs. **Marge : 3 comptes.** Pour un produit dont la raison
d'être est le suivi **multi-comptes**, cette marge n'est pas confortable — elle est simplement
non encore consommée.

**Scénarios d'exposition, classés** (correction utile à l'hypothèse du brief) :

1. **Reconnexion MQTT du tray, broker vivant** — `mqtt_sub.rs:39-43` re-souscrit à **chaque**
   `ConnAck` ; une nouvelle connexion = un nouveau `DataRequest` = `forward_retained = true` sur un
   retained **plein**. **C'est le scénario exposé.**
2. **Second abonné tardif** (sonde de debug, futur consommateur, analytics) — exposé, et c'est
   déjà ce qui a piégé le diagnostic.
3. **Démarrage de l'app** — **pas** le scénario exposé, contrairement à l'hypothèse : l'app
   embarque `iakahub`, le broker renaît **vide** avec elle, et le daemon le repeuple **en live**
   alors que le tray est déjà souscrit (chemin fan-out, celui qui passe à 300/300).

**Rattrapage si le seuil était franchi** : aujourd'hui le daemon republie ses 227 codes **à chaque
tick** — un abonné tronqué se répare en **≤ 60 s**. Après le **lot B** (publication différentielle),
seul le **resync complet périodique** (tous les 10 ticks) republie tout : le rattrapage passe à
**≤ 10 min**.

## Effet du lot B sur ce goulot — réponse directe

**Le lot B ne l'atténue pas, ne le crée pas, et dégrade d'un facteur 10 son unique filet.**

- **Sans effet sur la cause** : le backlog retained reste de 227 topics (39 sous les filtres du
  tray) quoi qu'il arrive — la dédup réduit le **débit par tick**, pas l'**état retenu** au broker.
- **Sans effet sur le seuil** : 7 topics par réservoir, inchangé.
- **Il déplace le rattrapage** : de « ≤ 1 tick » à « ≤ 10 ticks ». Le resync périodique du lot B
  cesse d'être un simple filet anti-divergence : il devient **le seul mécanisme de réparation** d'un
  abonné tronqué. À consigner, à ne pas rallonger.

## Options examinées (pour arbitrage)

| # | Option | Verdict |
|---|---|---|
| **O1** | **Ne rien changer au comportement ; garde-fou mesuré + documentation dans le projet** | ✅ **recommandée** — seule option qui ne touche pas le backbone et qui empêche le fait de pourrir |
| O2 | Relever `max_inflight_count` dans `iakahub/rumqttd.toml` | ❌ **inopérant** : la clé ne pilote pas cette fenêtre (constante de compilation). Gain nul sur le chemin produit (QoS 1), fausse sécurité maximale. Seul `max_outgoing_packet_count` a un effet — et **uniquement pour les abonnés QoS 0**, c.-à-d. les sondes de debug, pas le tray |
| O3 | Monter `rumqttd` 0.19 → 0.20 (sept. 2025), ou patcher | ❌ **pas de correctif amont** : `main` porte aujourd'hui encore la même troncature et le même `MAX_INFLIGHT = 100` (vérifié sur les sources amont). Forker le backbone pour ça = dette sans contrepartie |
| O4 | Découper les filtres d'abonnement du tray en filtres plus étroits | ❌ **fragile** : `free_slots()` est partagé **par connexion** ; découper répartit la même fenêtre de 100 au lieu de l'élargir, et rend le résultat dépendant de l'ordonnancement du routeur |
| O5 | Cesser de dépendre du retained pour l'état initial (dump demandé au daemon) | ❌ **sur-ingénierie aujourd'hui** — mais c'est la **vraie** issue si le nombre de comptes dépasse durablement 7. À cadrer à part le jour venu |

**Décision structurante à arbitrer par le décideur** : `iakahub` est un **backbone partagé**, pas un
composant privé de ce projet. O1 propose d'y toucher **une seule fois et sans effet d'exécution** :
un **commentaire** dans `iakahub/rumqttd.toml` disant ce que ces deux clés font réellement. Aucune
valeur modifiée, aucun changement de comportement. **Valider cette instruction, c'est valider cette
unique touche au backbone.** La refuser est cohérent : le commentaire irait alors dans le contrat
MQTT seul.

## Décision retenue

**O1 — rien à réparer, tout à outiller.** Trois gestes, aucun ne change le comportement du produit :

1. **Un garde-fou qui compte** : le nombre de topics d'un tick couverts par chaque filtre de
   consommateur devient une grandeur **calculée, testée et journalisée**, avec un seuil d'alerte
   sous le plafond. Le jour où un 8ᵉ compte est ajouté, le daemon le **dit**.
2. **Un test de caractérisation** qui mesure le plafond **contre le vrai broker** et fige les deux
   régimes (sous le seuil : tout arrive ; au-dessus : exactement 100, définitif). C'est la mesure
   avant/après de ce lot — non pas avant/après un correctif, mais **sous seuil / hors seuil**.
3. **La documentation du piège**, là où le contrat affirme aujourd'hui le contraire.

## Périmètre

**Inclus** :

- `iakatc-core` : les filtres de consommateur et le plafond deviennent des **constantes nommées du
  contrat**, plus des littéraux dispersés ; fonction pure de comptage par filtre ; tests unitaires.
- `src-tauri/src/mqtt_sub.rs` : consomme ces constantes au lieu de reconstruire les deux chaînes.
- `iakatc-daemon` : avertissement journalisé au tick quand un filtre approche le plafond.
- Un **test d'intégration de caractérisation** contre `iakahub::broker` sur **port libre**.
- `specs/contrat-mqtt-conso.md` § 4 : correction de l'affirmation fausse + section « plafond de
  rattrapage » + le piège des sondes.
- `iakahub/rumqttd.toml` : **commentaire seul** (sous réserve de l'arbitrage ci-dessus).

**Exclu — explicitement hors de ce lot** :

- **Toute modification de valeur** dans `iakahub/rumqttd.toml` (O2) — y compris
  `max_outgoing_packet_count`.
- **Toute montée de version ou tout patch de `rumqttd`** (O3).
- **Tout redécoupage des filtres d'abonnement du tray** (O4).
- **Toute alternative au retained** pour l'état initial (O5) — à cadrer à part si le seuil approche.
- Le **lot B** (publication différentielle) : cadré ailleurs, il n'est **pas** modifié ici. Ce lot
  se contente de consigner son effet sur le délai de rattrapage.
- Tout changement de `src-tauri/src/icon.rs`, `state.rs`, `history.rs`.
- Toute modification de `iakatc-daemon/src/mqtt.rs` : le transport est réparé, on n'y retouche pas.

## Étapes d'implémentation

1. **`iakatc-core/src/publish/contract.rs`** — exposer, à côté des constructeurs de topics :
   - `pub fn consumer_filters(root: &str) -> Vec<String>` (les **deux** filtres du tray, seule
     définition) ;
   - `pub const RETAINED_FANOUT_CEILING: usize = 100;` — le plafond dur de `rumqttd`, documenté
     avec sa référence de code (`router/iobufs.rs:18`, `router/routing.rs:1455-1467`) ;
   - `pub const RETAINED_BACKLOG_ALERT: usize = 80;` — seuil d'alerte (80 % du plafond) ;
   - `pub fn backlog_by_filter(messages: &[Message], filters: &[String]) -> Vec<(String, usize)>`
     — fonction **pure**, comptant les topics d'un lot couverts par chaque filtre. Utiliser
     `rumqttc::mqttbytes::matches` (public, `rumqttc-0.24.0/src/mqttbytes/topic.rs:63`) **ou** un
     matcher local de quelques lignes si l'on refuse d'ajouter `rumqttc` aux dépendances de `core`
     — **préférer le matcher local** : `core` ne dépend pas de `rumqttc` aujourd'hui et ce lot n'est
     pas le bon endroit pour l'y faire entrer.
2. **Tests unitaires de `core`** (mêmes fichier/module) :
   - le comptage sur un tick représentatif (5 réservoirs) donne **35** et **4** ;
   - aucun topic de conso ni de limits ne matche le filtre quota (le `+/+` ne franchit pas le
     segment `agents`) ;
   - à **15 réservoirs**, le filtre quota atteint **105** et dépasse `RETAINED_FANOUT_CEILING` —
     c'est ce test qui fige le seuil de rupture ;
   - à 5 réservoirs, on est sous `RETAINED_BACKLOG_ALERT`.
3. **`src-tauri/src/mqtt_sub.rs`** — remplacer les deux `format!` de `mqtt_sub.rs:33-34` par
   `consumer_filters(&cfg.root)` et souscrire à chacun. Aucun changement de comportement attendu :
   les chaînes produites doivent être **identiques**. Rendre `mqtt_sub` public dans
   `src-tauri/src/lib.rs:11` si le test l'exige — sinon **ne pas y toucher**.
4. **`iakatc-daemon/src/main.rs`** — après `publish_batch`, appeler `backlog_by_filter` sur les
   messages du tick et **journaliser un avertissement explicite** pour tout filtre dont le compte
   atteint `RETAINED_BACKLOG_ALERT` : nommer le filtre, le compte, le plafond, et la conséquence
   (« un abonné qui se reconnecte ne recevra qu'une partie de son état initial »). Sous le seuil :
   **rien** — pas de bruit au log à chaque tick.
5. **Test d'intégration de caractérisation** — `iakahub/tests/retained_backlog_ceiling.rs`
   (voisin de `broker_roundtrip.rs`, dont on reprend le motif `free_port()`) :
   - **jamais sur 1883** : port libre obtenu comme dans `broker_roundtrip.rs` ; aucune publication
     sur le broker de l'app installée ;
   - **cas sous le seuil** : publier **39** topics retained sous le filtre quota **noyés dans 227
     topics retained au total**, déconnecter le publisher, **puis** connecter un abonné posant les
     **mêmes filtres que le tray**, en QoS 1 ⇒ **39/39 reçus** ;
   - **cas hors seuil** : publier **150** topics retained sous **un seul** filtre, déconnecter,
     connecter un abonné ⇒ **exactement 100** reçus, et le compte **n'évolue plus** après une
     attente bornée (≥ 3 s). C'est la preuve que la troncature est définitive, pas un débit bridé ;
   - **un seul filtre** dans le cas hors seuil (cf. point 4 de la cause racine : deux filtres se
     partagent la fenêtre et rendraient le « exactement 100 » flou).
6. **`specs/contrat-mqtt-conso.md` § 4** :
   - **corriger** la phrase de `contrat-mqtt-conso.md:205-207` — « le sert **immédiatement** à tout
     nouvel abonné » est **faux au-delà de 100 topics par filtre** sur `rumqttd` ;
   - ajouter un encadré **« Plafond de rattrapage du retained »** : la règle (100 en QoS 1, 200 en
     QoS 0, par filtre, à l'abonnement, sous-ensemble arbitraire, définitif), la référence de code,
     le fait que **`max_inflight_count` ne le pilote pas**, le seuil de rupture (15 réservoirs /
     8 comptes) et le mécanisme de rattrapage (resync complet du daemon) ;
   - ajouter le **piège de sondage** : *un total qui tombe pile sur 100 ou 200 en sondant `#` est
     un artefact de transport, pas un inventaire de topics* — avec le geste sûr (compter côté
     **broker**, ou sonder par filtres étroits, ou lire l'état du daemon).
7. **`iakahub/rumqttd.toml`** (si O1 validé dans son intégralité) — **commentaire seul**, au-dessus
   des deux clés : ce que `max_inflight_count` fait réellement (tampon réseau), ce que
   `max_outgoing_packet_count` fait réellement (QoS 0 seulement), et que la fenêtre sortante QoS ≥ 1
   est une constante de compilation de `rumqttd`, non configurable. **Aucune valeur modifiée.**

## Fichiers concernés

- `iakatc-core/src/publish/contract.rs` — filtres de consommateur, plafond, seuil, comptage + tests.
- `src-tauri/src/mqtt_sub.rs` — consomme les filtres du contrat (lignes 33-34, 42-43).
- `src-tauri/src/lib.rs` — visibilité de `mqtt_sub` **si et seulement si** le test l'exige.
- `iakatc-daemon/src/main.rs` — avertissement de seuil au tick (après `mqtt.rs`-`publish_batch`).
- `iakahub/tests/retained_backlog_ceiling.rs` — **nouveau** : caractérisation sous seuil / hors seuil.
- `iakahub/Cargo.toml` — dev-dependency `iakatc-core` (chemin) + `rumqttc` pour l'abonné du test.
- `specs/contrat-mqtt-conso.md` — § 4 : correction + encadré plafond + piège de sondage.
- `iakahub/rumqttd.toml` — **commentaires seuls**, aucune valeur.
- **Non modifiés** : `iakatc-daemon/src/mqtt.rs`, `iakatc-core/src/quota/*`, `src-tauri/src/icon.rs`,
  `src-tauri/src/state.rs`, `iakahub/src/broker.rs`.

## Risques

- **Test de caractérisation instable (risque principal).** Le cas « hors seuil » affirme une
  **absence** (« plus rien n'arrive »), ce qui se prouve mal. *Mitigation* : attente bornée et
  généreuse (≥ 3 s après le dernier message), assertion sur `== 100` et non sur un intervalle, un
  seul filtre, port libre, et **aucune publication concurrente** pendant la fenêtre d'observation.
  Si le test se révèle flaky à l'exécution, le **marquer `#[ignore]` avec justification** plutôt
  que d'affaiblir l'assertion — un test qui ment est pire que pas de test (leçon du lot A).
- **Fausse alerte du garde-fou.** Le compte est calculé sur les messages d'**un tick** ; avec le
  lot B (dédup), un tick ne portera plus tous les topics. *Mitigation* : compter sur le **lot
  complet du contrat** (`tick_messages`), pas sur ce qui a été effectivement publié — et l'écrire
  dans le code, sinon le lot B rendra le garde-fou aveugle sans que personne ne le voie.
- **Dérive des filtres.** Si le tray et le contrat redivergent, le garde-fou surveille un filtre que
  personne n'utilise. *Mitigation* : **une seule** définition (`consumer_filters`), consommée par le
  tray ; un test qui compare les chaînes produites aux chaînes historiques.
- **Couplage de couche.** `iakahub` (backbone) prend `iakatc-core` en **dev-dependency**. Assumé et
  borné aux tests ; si cela dérange, déplacer le test dans `iakatc-daemon/tests/` avec `iakahub` en
  dev-dependency (inversion symétrique, même coût).
- **Broker de production touché.** Le vrai broker écoute sur `127.0.0.1:1883` avec l'app en service.
  *Mitigation* : `free_port()` obligatoire, jamais de port fixe, jamais de publication sur 1883 —
  critère de revue explicite.
- **Le plafond bouge chez l'amont.** Si `rumqttd` corrige un jour la troncature, le test de
  caractérisation **échouera** — c'est voulu : il rouvre le sujet au lieu de laisser une doc
  périmée. Le message d'échec doit le dire.

## Comportement attendu

- Aucun changement fonctionnel visible : les jauges du tray se comportent exactement comme
  aujourd'hui.
- Le log du daemon reste **silencieux** sur ce sujet tant que les filtres sont sous 80 topics, et
  devient **explicite** au-delà.
- Le contrat MQTT cesse d'affirmer que le broker sert le retained « immédiatement à tout nouvel
  abonné » sans réserve.
- Le prochain qui sonde ce broker et lit 100 ou 200 sait, en une ligne de contrat, que c'est un
  artefact.

## Vérification

### Critères d'acceptation

- [ ] **C1 (exposition du tray, mesurée)** — test unitaire : sur le lot de tick représentatif
      (5 réservoirs, 44 projets), `backlog_by_filter` rend **exactement** `35` pour
      `{root}/all/ia/+/+/quota/#` et `4` pour `{root}/meta/daemon/#`, soit **39 sur 227**. Le
      chiffre est consigné dans le message de commit.
- [ ] **C2 (seuil de rupture, figé)** — test unitaire : à **14** réservoirs le filtre quota vaut
      `98` (< 100) ; à **15** il vaut `105` (> 100). Le test nomme la conséquence : 8ᵉ compte IA.
- [ ] **C3 (non-couverture)** — test unitaire : aucun topic de `conso_project_agent`,
      `conso_provider_agent` ni `limits` ne matche les filtres du tray.
- [ ] **C4 (mesure sous le seuil, vrai broker)** — test d'intégration sur port libre : 227 topics
      retained publiés, dont 39 sous les filtres du tray ; un abonné QoS 1 posant **les filtres du
      tray** après coup reçoit **39/39**. C'est la preuve que le tray n'est pas exposé aujourd'hui.
- [ ] **C5 (mesure hors seuil, vrai broker)** — même dispositif, **150** topics retained sous **un**
      filtre : l'abonné reçoit **exactement 100**, et le compte est **encore 100** après ≥ 3 s
      d'attente supplémentaire. C'est la preuve que la troncature est définitive.
- [ ] **C6 (garde-fou vivant)** — le daemon journalise un avertissement nommant le filtre, le
      compte et le plafond dès qu'un filtre atteint 80 ; et **ne journalise rien** en dessous
      (vérifié sur le tick courant : aucun bruit ajouté).
- [ ] **C7 (source unique des filtres)** — `mqtt_sub.rs` ne contient plus de littéral de filtre ;
      les chaînes souscrites sont **identiques** à celles d'avant (test de non-régression sur les
      deux chaînes exactes).
- [ ] **C8 (contrat)** — `specs/contrat-mqtt-conso.md` § 4 ne contient plus l'affirmation non
      réservée de la ligne 205-207, porte l'encadré « plafond de rattrapage » avec ses références de
      code, et le piège de sondage 100/200.
- [ ] **C9 (backbone intact)** — `git diff iakahub/rumqttd.toml` ne montre que des lignes de
      **commentaire** ; aucune valeur de configuration modifiée. Critère de revue explicite.
- [ ] **C10 (hygiène)** — aucun test n'ouvre ni ne publie sur `127.0.0.1:1883` ; tous passent par
      `free_port()`. Critère de revue explicite.
- [ ] `cargo fmt` / `cargo clippy` propres, `cargo test` vert sur tout le workspace
      (`bash scripts/quality-report.sh`).

### Ce qui fait échouer ce lot

Livrer une modification de valeur dans `iakahub/rumqttd.toml` « pour être tranquille ». Ce serait
inopérant (C9 l'interdit, la cause racine l'explique) et cela substituerait une croyance à une
mesure — précisément ce que ce lot existe pour empêcher.

---

## Sources externes consultées

- Troncature du retained à l'abonnement et `forward_retained = false` :
  `rumqttd-0.19.0/src/router/routing.rs:1455-1467` (source vendue dans le registre cargo local),
  **identique sur le `main` amont** —
  https://raw.githubusercontent.com/bytebeamio/rumqtt/main/rumqttd/src/router/routing.rs
- `MAX_INFLIGHT: usize = 100` constante de compilation et `free_slots()` :
  `rumqttd-0.19.0/src/router/iobufs.rs:18,101-103`, **inchangé sur le `main` amont** —
  https://raw.githubusercontent.com/bytebeamio/rumqtt/main/rumqttd/src/router/iobufs.rs
- `max_inflight_count` de la configuration passé en `max_connection_buffer_len` de `Network` :
  `rumqttd-0.19.0/src/server/broker.rs:498-503` et `rumqttd-0.19.0/src/link/network.rs:43-58`
- Lecture du retained sur un `HashMap` (ordre non spécifié) :
  `rumqttd-0.19.0/src/router/logs.rs:61,272-307`
- Reprise du chemin live sur PUBACK : `rumqttd-0.19.0/src/router/scheduler.rs:163`
- Dernière version publiée de `rumqttd` : **0.20.0 (29 septembre 2025)**, 0.19.0 datant du
  12 décembre 2023 — https://crates.io/api/v1/crates/rumqttd
- Symptôme connu et public du même plafond côté abonné (retained tronqués à ~100 + inflight) —
  https://github.com/bytebeamio/rumqtt/issues/170
- `rumqttc::mqttbytes::matches` est public : `rumqttc-0.24.0/src/mqttbytes/topic.rs:63`
