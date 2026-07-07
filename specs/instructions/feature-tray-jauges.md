# Instruction : GUI tray + jauges de réservoir (subscriber MQTT)

> Rédigé par Gandalf (P1 — cadrage). Consommé par Gimli comme instruction de travail.
> Deuxième brique du produit : la **face visible** (moniteur tray) qui **consomme** les codes
> retained publiés par le daemon v0. **Interface d'entrée figée** par `specs/contrat-mqtt-conso.md`
> (à lire AVANT) ; le daemon est figé par `specs/instructions/feature-collecteur-logs.md`.

---

## Contexte

iakaTokenCounter est un **moniteur de consommation IA multi-comptes en tray** (cf. `PROJET.md`
§ Vision + Cap). Le **daemon de mesure v0** (instruction frère, en cours d'implémentation par Gimli)
**mesure et publie** dans le broker Mosquitto d'iakalogs des **codes scalaires retained**
(`{"v":…,"t":…}`) selon deux axes et par compte/fenêtre. Il manque la **face visible** : une **app
Tauri v2** qui pose une **icône de barre système** et **affiche une jauge « réservoir » par compte
IA** (quota restant 5 h + 7 j), avec le **niveau de confiance** de chaque valeur.

Cette instruction ferme le périmètre de la **GUI tray (subscriber) MVP**. Elle **ne recalcule
rien** : elle **lit** les codes retained du contrat et les **rend**. Elle **ferme aussi** le point
laissé ouvert par l'instruction daemon : **comment le daemon tourne par rapport à la GUI** (D1).

### Faits vérifiés (veille Gandalf, sources en bas)

- **Tauri v2 — tray** : `tauri::tray::TrayIconBuilder` (dans le hook `setup`) crée l'icône ; menu via
  `MenuBuilder`/`MenuItemBuilder` ; `AppHandle::tray_by_id` pour la retrouver. On peut afficher/
  masquer une fenêtre (`app.get_webview_window("popover")` → `.show()`/`.hide()`), régler le tooltip/
  titre, et **désactiver le menu au clic gauche** via `show_menu_on_left_click(false)` (pour réserver
  le clic gauche à l'ouverture de la popover).
- **Tauri v2 — sidecar** : un binaire compagnon se déclare dans `tauri.conf.json` sous
  `bundle.externalBin` ; il **doit exister avec le suffixe `-$TARGET_TRIPLE`** (ex.
  `iakatc-daemon-aarch64-apple-darwin`). Côté Rust, `tauri_plugin_shell::ShellExt` →
  `app.shell().sidecar("iakatc-daemon")` puis `.spawn()` (lit stdout, gère l'arrêt). C'est le
  mécanisme standard pour embarquer et lancer le daemon avec la GUI.
- **Contrat MQTT** (résumé utile ici) : la GUI **s'abonne** en retained à
  `iakatokencounter/all/ia/+/+/quota/#` (jauges) et lit, par `(provider, account, window∈{5h,7d})`,
  les codes `used_pct`, `remaining_pct`, `resets_at`, `confidence`, `source` (+ `used_tokens`
  diagnostic). Payload = `{"v":<scalaire>,"t":<epoch_s>}`. `v:null` = « inconnu daté ». QoS 1.
  Broker `192.168.2.11:1883`, config par env (§ 6 du contrat).

## Ce qui existe

| Élément | Où | État |
|---|---|---|
| Vision + Cap + décisions | `specs/PROJET.md` | à jour |
| **Contrat MQTT** (codes consommés) | `specs/contrat-mqtt-conso.md` | figé |
| Daemon de mesure v0 (producteur) | `specs/instructions/feature-collecteur-logs.md` | figé ; **en cours** (Gimli) |
| Binaire `iakatc-daemon` | crate `iakatc-daemon` | produit par l'instruction daemon |
| Stack décidée | Tauri v2 (Rust + webview TS) | `PROJET.md` § Stack |
| GUI tray / fenêtre / subscriber | — | **absente (objet de cette instruction)** |

## Décision

### D1 — Câblage daemon ↔ GUI : **le daemon est un sidecar spawné par la GUI** (point fermé)

**Retenu** : la GUI Tauri **embarque `iakatc-daemon` en sidecar** (`bundle.externalBin` + suffixe
target-triple) et le **spawne au démarrage** via `tauri-plugin-shell` (`ShellExt::sidecar`). La GUI
et le daemon **ne se parlent QUE via le broker** (le daemon publie, la GUI subscribe) — jamais en
direct. Un flag d'env **`IAKATC_SPAWN_DAEMON` (défaut `true`)** permet de **ne pas** spawner quand
un daemon géré par le système (service/launchd) tourne déjà.

**Pourquoi (a) sidecar et pas (b) daemon lancé séparément** :
- **UX standalone** (exigence `PROJET.md` : « runnable en standalone, sans rien du projet iaka ») :
  l'utilisateur lance **une seule chose** (l'app tray) ; le daemon suit. Le produit *est* le tray.
- **Découplage préservé** : toute la communication passe par le contrat MQTT retained → la GUI reste
  un pur subscriber, le daemon reste figé, aucun couplage de code.
- **Mécanisme standard** : `externalBin` + `ShellExt::sidecar` est la voie documentée Tauri v2.

**Écarté / nuance** :
- *(b) daemon séparé uniquement* : plus simple à **builder** (pas d'empaquetage sidecar) mais impose
  à l'utilisateur de lancer/superviser 2 process → mauvaise UX pour un moniteur grand public.
- **Limite assumée** : en mode sidecar, **le daemon s'arrête avec la GUI**. Acceptable au MVP (un
  moniteur tray est censé rester ouvert). Pour un déploiement **serveur/headless** (daemon publiant
  pour IakaCockpit sans GUI), on met `IAKATC_SPAWN_DAEMON=false` et on lance le daemon en service —
  **hors scope build ici**, juste prévu par le flag.

> **Micro-choix tranché** : sidecar par défaut, avec échappatoire `IAKATC_SPAWN_DAEMON=false`. Si le
> spawn échoue (binaire absent), la GUI **ne crashe pas** : elle passe en subscriber pur et signale
> « daemon indisponible » (voir D5).

### D2 — La GUI est un **pur subscriber** ; l'état vit dans le backend Rust, la webview rend

**Retenu** : le **backend Rust** de la GUI tient un **client rumqttc** (event-loop tokio) abonné aux
topics de jauge, maintient un **état en mémoire indexé par topic-code** (dernière valeur retained
reçue), et **pousse les changements vers la webview** via un **événement Tauri** (`tray://state`).
La webview expose aussi une **commande de snapshot** (`get_reservoirs`) pour l'état initial à
l'ouverture de la popover. **La webview ne parle jamais à MQTT directement.**

**Pourquoi** : le contrat impose « le client lit la valeur sans recalculer ». Le backend Rust est le
seul endroit avec accès réseau/MQTT (la webview n'a pas de socket TCP). rumqttc est déjà le client
retenu côté daemon → cohérence de stack. Le retained garantit qu'à la (re)connexion la GUI reçoit
**immédiatement** les dernières valeurs sans rejeu.

**Écarté** : *MQTT-over-WebSocket depuis la webview* (port `9883` du broker) — possible mais ajoute
un client MQTT JS + gestion d'auth côté front ; inutile puisque le backend Rust le fait déjà et plus
proprement. (Laissé comme évolution éventuelle, hors scope.)

### D3 — Modèle de « réservoir » consommé et regroupement par compte

La GUI **découvre les comptes dynamiquement** via l'abonnement générique
`iakatokencounter/all/ia/+/+/quota/#` : chaque `(provider, account)` rencontré devient une **carte de
réservoir**, avec **deux jauges** (fenêtre `5h` et `7d`). Pour chaque jauge, la GUI lit :

| Donnée affichée | Code source (contrat) |
|---|---|
| Remplissage de la jauge (restant) | `.../quota/{window}/remaining_pct/current` |
| Étiquette « utilisé » | `.../quota/{window}/used_pct/current` |
| Compte à rebours de recharge | `.../quota/{window}/resets_at/current` |
| **Badge de confiance** | `.../quota/{window}/confidence/current` |
| Provenance (tooltip) | `.../quota/{window}/source/current` |

**Rendu de la confiance (D3.1)** — mapping visuel MVP (ajustable) :

| `confidence` | Rendu jauge |
|---|---|
| `official` | plein, teinte « sûr » (vert) |
| `official_stale` | plein + pictogramme « horloge » (ambre) — valeur datée |
| `local_estimate` | hachuré + `~` devant le % (bleu) — estimation |
| `none` | grisé, `?` à la place du % (`v:null`) |

> `remaining_pct = v` du code `remaining_pct/current` ; si `v` est `null` → jauge grisée `?`.
> Le **countdown** dérive de `resets_at` (epoch s) − `now`. La jauge est considérée **périmée** si le
> `t` du payload dépasse un seuil de fraîcheur local **ou** si `now > resets_at` (contrat § 4).

### D4 — Représentation tray : icône simple + popover (pas de dessin fin dans l'icône)

**Retenu** (confirme le défaut acté `PROJET.md`) : **icône statique** dans le tray + **tooltip**
résumant le **pire réservoir** (plus petit `remaining_pct` parmi tous les comptes, ex. « Claude 5h :
12 % »). **Clic gauche → ouvre/masque une popover** (petite fenêtre Tauri) contenant **la liste des
cartes de réservoir** (une par compte, deux jauges). **Menu clic droit** : « Ouvrir », « Quitter »
(`show_menu_on_left_click(false)` pour réserver le clic gauche à la popover).

**Pourquoi** : dessiner un remplissage fin **dans** l'icône est le point de risque cross-OS (rendu
d'icône hétérogène macOS/Windows/Linux). Le neutraliser au MVP = icône fixe, détail dans la popover.

**Écarté** : *jauge dessinée dans l'icône* (canvas → PNG dynamique par plateforme) → repoussé.

### D5 — Dégradation hors-ligne : « inconnu », jamais de crash

- **Broker injoignable** (au démarrage ou perte en cours) : la GUI **reste ouverte**, affiche les
  jauges en **état « inconnu »** (grisé) si aucune valeur retained n'a encore été reçue, ou **la
  dernière valeur connue marquée « périmée »** sinon, plus un **indicateur global « broker
  déconnecté »**. rumqttc **retente** en tâche de fond (backoff) ; à la reconnexion, le retained
  **repeuple** les jauges automatiquement.
- **Spawn daemon échoué** (D1) : bandeau « daemon indisponible » ; la GUI continue en subscriber pur
  (utile si un daemon tourne ailleurs sur le broker).
- Broker **configurable** par les **mêmes variables d'env que le contrat** (`IAKATC_MQTT_HOST/PORT/
  USER/PASSWORD/ROOT`, § 6) → la GUI et le daemon lisent la même config.

### D6 — Hook analytics (double-clic) : **point d'entrée seulement, pas d'analytics ici**

On **prévoit** le point d'entrée sans l'implémenter : un **double-clic sur une carte de réservoir**
(dans la popover) déclenche une commande Tauri **`open_analytics(account)`** qui, au MVP, est un
**stub** (ex. ouvre une fenêtre vide « À venir » ou no-op journalisé). L'app d'analytics (courbes,
historique, logs) fait l'objet de `feature-app-analytics.md` (3ᵉ instruction). **Aucune logique
d'historique n'est écrite ici.**

> **Micro-choix tranché** : le hook est un **double-clic sur la carte** (cohérent avec la vision
> « double-clic sur les barres → app analytics ») ; le stub est une fenêtre « À venir » plutôt qu'un
> no-op muet, pour rendre le hook **observable** en test.

## Étapes d'implémentation

1. **Scaffold Tauri v2** à la racine (front TS minimal + `src-tauri`), plugins `tauri-plugin-shell`
   (sidecar) et le nécessaire tray. Cible **macOS + Windows + Linux**.
2. **Client MQTT subscriber** (`src-tauri/src/mqtt_sub.rs`) : rumqttc, config par env (D5),
   abonnement `iakatokencounter/all/ia/+/+/quota/#` (+ `meta/daemon/#` pour l'état daemon), QoS 1,
   reconnexion/backoff. Parse `{"v":…,"t":…}` (défensif : payload malformé ignoré).
3. **État en mémoire + événement** (`src-tauri/src/state.rs`) : map `topic-code → (v, t)` ;
   agrégation en `Reservoir[]` par `(provider, account, window)` ; émission `tray://state` sur
   changement ; commande `get_reservoirs()` (snapshot) et `open_analytics(account)` (stub, D6).
4. **Spawn daemon sidecar** (D1) : dans `setup`, si `IAKATC_SPAWN_DAEMON != "false"`,
   `app.shell().sidecar("iakatc-daemon").spawn()` ; échec → bandeau « daemon indisponible », pas de
   crash. Déclarer `bundle.externalBin` + convention de nommage target-triple.
5. **Tray** (`src-tauri/src/tray.rs`) : `TrayIconBuilder` dans `setup`, tooltip = pire réservoir,
   `show_menu_on_left_click(false)`, clic gauche → toggle popover, menu droit « Ouvrir »/« Quitter ».
6. **Popover / fenêtre** : fenêtre Tauri `popover` (petite, sans décor), rend les cartes de réservoir
   depuis `tray://state` + snapshot initial ; double-clic carte → `open_analytics`.
7. **Rendu front** (TS) : composant « carte réservoir » (2 jauges 5h/7d), mapping confiance→style
   (D3.1), countdown depuis `resets_at`, états « inconnu »/« périmé »/« broker déconnecté » (D5).
8. **Config** : lecture des mêmes env que le contrat ; documenter dans le README de l'app.
9. **Mocks/tests** : fixtures de messages retained (valeurs `official`/`stale`/`estimate`/`none`,
   `v:null`, plusieurs comptes) injectables dans l'agrégateur d'état **sans broker réel** (interface
   de source substituable, comme côté daemon).
10. **README GUI** : env broker, `IAKATC_SPAWN_DAEMON`, empaquetage sidecar (target-triples),
    comportement hors-ligne, renvoi au contrat, note du hook analytics.

## Fichiers concernés

- `src-tauri/src/mqtt_sub.rs` — subscriber rumqttc (env, abonnements, reconnexion).
- `src-tauri/src/state.rs` — état en mémoire, agrégation `Reservoir[]`, événements + commandes.
- `src-tauri/src/tray.rs` — `TrayIconBuilder`, tooltip, clic gauche/menu.
- `src-tauri/src/lib.rs` (ou `main.rs`) — `setup` : tray + spawn sidecar (D1) + registre commandes.
- `src-tauri/tauri.conf.json` — fenêtre `popover`, `bundle.externalBin` (daemon), capabilities shell.
- `src/…` (front TS) — composant carte réservoir + jauges + mapping confiance + états dégradés.
- `src-tauri/tests/…` ou modules `#[cfg(test)]` — agrégation d'état sur fixtures.
- `README.md` (app) — config, sidecar, hors-ligne, hook analytics.

## Comportement attendu

Critères **observables et testables** :

- Au démarrage avec broker joignable, l'app **pose une icône dans le tray** (macOS + Windows +
  Linux) et le **clic gauche ouvre/masque la popover**.
- Un message retained `.../ia/claude/max/quota/5h/remaining_pct/current` = `{"v":87.5,"t":…}` fait
  apparaître une **carte « claude / max »** avec une **jauge 5h remplie à 87,5 %** ; un message
  `7d/remaining_pct` ajoute la **seconde jauge**.
- Un code `.../confidence/current` = `{"v":"official",…}` → jauge en style « sûr » ; `official_stale`
  → pictogramme horloge ; `local_estimate` → hachuré + `~` ; `none` (ou `remaining_pct` `v:null`) →
  **jauge grisée avec `?`**.
- Deux comptes distincts (`(claude,max)` et `(claude,pro)`, ou `(codex,default)`) produisent **deux
  cartes** distinctes (découverte dynamique via le wildcard).
- Le **tooltip** du tray reflète le **plus petit `remaining_pct`** parmi les comptes.
- **Broker coupé** : l'app **reste ouverte**, affiche l'indicateur « broker déconnecté », garde les
  dernières valeurs marquées **« périmées »** (ou « inconnu » si jamais reçues) ; **à la reconnexion**
  les jauges se **repeuplent** sans redémarrage (retained).
- **`IAKATC_SPAWN_DAEMON=false`** : l'app **ne spawne pas** le daemon et fonctionne en subscriber pur ;
  spawn en échec (binaire absent) → bandeau « daemon indisponible », **pas de crash**.
- **Double-clic** sur une carte déclenche `open_analytics(account)` → ouverture de la fenêtre stub
  « À venir » (hook observable, sans logique analytics).
- Un payload MQTT **malformé** (pas `{"v","t"}`) est **ignoré**, sans figer ni planter la GUI.
- L'agrégateur d'état, alimenté par des **fixtures** (sans broker), produit le `Reservoir[]` attendu
  (test unitaire : mapping codes → cartes/jauges/confiance).

## Vérification

- [ ] `cargo check` / typecheck front OK (Tauri build)
- [ ] `cargo clippy` + lint front OK
- [ ] `cargo test` vert : agrégation d'état sur fixtures (confiance, `v:null`, multi-comptes, payload
      malformé ignoré)
- [ ] Build Tauri des 3 cibles (macOS, Windows, Linux) réussi (ou au moins l'OS de dev + note CI)
- [ ] Testé dans l'app réelle par le développeur : daemon (sidecar) publiant, tray visible, popover
      avec jauges réelles depuis un vrai compte Pro/Max, coupure/reprise du broker, double-clic → stub

## Hors scope

- **App d'analytics / historique** (courbes, logs, agrégats par période) ouverte au double-clic
  → `feature-app-analytics.md` (3ᵉ instruction). Ici : **seulement le hook** `open_analytics` (stub).
- **Toute modification du daemon** `iakatc-daemon` (figé par son instruction) et du **contrat MQTT**.
- **Vendoring / abonnement dans IakaCockpit** (Cockpit s'abonne de son côté au même contrat).
- **Persistance CouchDB** des métriques et **routage des conversations**.
- **Providers autres que Claude Code + Codex** (déjà bornés par le contrat/daemon).
- **Dessin fin de la jauge dans l'icône** tray (repoussé, D4) et **MQTT-over-WebSocket dans la
  webview** (repoussé, D2).
- **Déploiement du daemon en service headless** (le flag `IAKATC_SPAWN_DAEMON=false` le prévoit, mais
  l'empaquetage service n'est pas construit ici).

---

## Sources (veille)

- Tauri v2 — System Tray (`TrayIconBuilder`, menu, `show_menu_on_left_click`) :
  https://v2.tauri.app/learn/system-tray/
- Tauri v2 — Embedding External Binaries / sidecar (`externalBin`, target-triple, `ShellExt`) :
  https://v2.tauri.app/develop/sidecar/
- rumqttc (client MQTT Rust, subscribe + retained + QoS) : https://crates.io/crates/rumqttc
- Contrat des codes consommés (topics/payloads/retained) : `specs/contrat-mqtt-conso.md`
- Vision + décisions (icône simple + jauges popover, multi-OS) : `specs/PROJET.md`
