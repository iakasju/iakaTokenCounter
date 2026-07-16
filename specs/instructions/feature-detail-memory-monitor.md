# Instruction : Courbe de monitoring mémoire (RAM) — historique persistant

> Rédigé par Gandalf (P1 — cadrage). Consommé par Gimli (Claude Code) comme instruction de
> travail. Doc en français, code/identifiants en anglais.
> Périmètre : app tray Tauri v2 (`src-tauri/` + webview `src/`). Aucune modif du contrat MQTT,
> du daemon, ni de `iatc-core`.
>
> **Révision (décideur) :** la première version cadrait une courbe **live volatile** (buffer webview,
> repart de zéro à chaque ouverture). Stéphane a tranché : la courbe doit être **PERSISTANTE** —
> l'historique mémoire survit à la fermeture de la fenêtre **et** à un redémarrage du tray. Ce
> document remplace le périmètre live par un périmètre **échantillonnage continu + stockage disque**.

---

## Contexte

Stéphane (décideur) demande d'**ajouter dans la fenêtre de détails du counter** (la vue analytics
ouverte au double-clic sur un compte : `analytics.html` + `src/analytics.ts`) une **courbe de
monitoring de la RAM** du poste (macOS / Windows / Linux), **persistante dans le temps**.

Aujourd'hui cette fenêtre montre le quota courant du compte (en-tête) + trois visualisations
d'historique de tokens (timeline, treemap, split). Le tray, lui, **tourne en permanence en
headless** (app `Accessory` sur macOS, `skipTaskbar` ailleurs) : il y a donc un process hôte
disponible **en continu** pour échantillonner la RAM, indépendamment de l'ouverture de la fenêtre.
C'est ce que la persistance exige : **le sampler vit dans le process tray**, pas dans la webview.

Cette métrique système est **orthogonale** au produit (conso IA / quota) : widget de confort
d'observabilité local, sans rapport avec MQTT ni la mesure de tokens.

## Faits vérifiés (état de l'art, à jour — juillet 2026)

- **`sysinfo` n'est PAS déjà une dépendance.** `src-tauri/Cargo.toml` ne dépend que de `tauri`,
  `tauri-plugin-shell`, `resvg`, `rumqttc`, `serde`, `serde_json`, `iakatc-core`. → il faut
  **ajouter une dépendance** (ajout justifié et minimal ; pas de « réutiliser l'existant » possible
  pour la RAM).
- **`sysinfo` = choix retenu.** Crate de facto pour l'info système en Rust, **cross-OS
  macOS + Windows + Linux** (couvre la cible multi-OS), pure-Rust côté API.
  - Dernière version : **0.39.3** (publiée le 2026-05-28).
  - **MSRV = rustc 1.95** → **à VÉRIFIER** avant de figer la version (étape 1). Si la toolchain est
    antérieure, épingler une version plus ancienne de `sysinfo` plutôt que bumper la toolchain (MVP).
  - API mémoire : `System::total_memory()` / `System::used_memory()`, **en octets** ; refresh ciblé
    (`RefreshKind`/`MemoryRefreshKind`) pour ne rafraîchir **que** la mémoire (pas l'énumération des
    process, coûteuse). `used_memory()` (sysinfo récent) = RAM réellement utilisée (hors
    cache/buffers) → ratio `used/total` cohérent.
  - Conf minimale visée : `default-features = false, features = ["system"]` (le multi-threading par
    défaut sert l'énumération des process et **accroît la conso mémoire sur macOS** ; on n'en a pas
    besoin). Confirmer le nom exact de la feature contre la doc 0.39.3 à l'implémentation.
- **Stockage persistant = répertoire de données de l'app (Tauri v2, sans plugin).** Le `Manager`
  Tauri v2 expose `app.path().app_data_dir()` → renvoie un `PathBuf` propre à l'app
  (`identifier = com.iakaframe.iakatokencounter`, cf. `tauri.conf.json`) : macOS `~/Library/Application
  Support/com.iakaframe.iakatokencounter/`, Linux `$XDG_DATA_HOME` (ou `~/.local/share/...`),
  Windows `%LOCALAPPDATA%\...`. **À créer** (`create_dir_all`) avant écriture. Pas de plugin ni de
  base de données à ajouter — on réutilise `serde_json` (déjà présent) et un simple fichier.
- Sources : voir bloc « Sources » en fin de restitution Gandalf.

---

## Ce qui existe (à réutiliser)

| Élément | Où | Rôle pour cette feature |
|---|---|---|
| Modèle de **tâche de fond** du tray | `src-tauri/src/mqtt_sub.rs::start` (`std::thread::spawn`), lancé en `setup()` (`lib.rs:52`) | **Patron du sampler** : thread dédié détaché, lancé au `setup`, qui ne panique jamais |
| Push d'un **événement** vers la webview | `mqtt_sub.rs::push_state` → `app.emit("tray://state", …)` (`mqtt_sub.rs:83-89`) | Patron pour émettre `tray://memory` (croissance live sans polling) |
| Arrêt du sidecar en `RunEvent::Exit` | `lib.rs:66-74` | Le sampler est un thread détaché : meurt avec le process, rien à ajouter |
| État partagé managé | `src-tauri/src/state.rs::AppState` (managé `lib.rs:26`) | Y ajouter le verrou + chemin du log mémoire |
| Commande read-only modèle | `src-tauri/src/history.rs::get_history` | Patron de `get_memory_history` (payload serde camelCase) |
| Enregistrement des commandes | `lib.rs:27-31` `generate_handler!` | Y ajouter `get_memory_history` |
| Visualisations SVG « maison » (aucune lib) | `src/history.ts` (helpers purs `svg()`, `el()`, `emptyState()`, `fmtTokens()`) | Base du line chart mémoire |
| Vue de détails | `analytics.html`, `src/analytics.ts` | Y brancher la section mémoire + écoute d'événement |
| Types de rendu | `src/types.ts` | Y ajouter `MemorySample` |
| Répertoire de données app | `app.path().app_data_dir()` (Tauri v2, `Manager`) | Emplacement du fichier persistant |
| Harnais de test TS | `package.json` | ❌ aucun → TS vérifié par `typecheck` + test réel ; tests unitaires côté **Rust** (fonctions pures I/O) |

---

## Décision

### D1 — Métrique : **RAM utilisée en % (used/total)** comme courbe, absolu (Go) en appoint

- Courbe = pourcentage `used/total × 100`, axe Y **fixe 0–100 %** (borne stable, comparable entre
  postes, robuste cross-OS).
- Valeurs absolues en complément (pas une 2ᵉ courbe) : **readout courant** « `62 %` · `9,8 / 16 Go` »
  + **tooltip SVG `<title>`** par point (`hh:mm` · `62 %` · `9,8 Go`), comme les `<title>` de
  `history.ts`. Conversion octets → Go décimaux (`/ 1e9`) côté **webview** (présentation).

### D2 — Source : `sysinfo`, lecture d'un échantillon `{t, usedBytes, totalBytes}`

- Ajout de `sysinfo` (cf. Faits vérifiés), conf mémoire seule. Une fonction Rust lit un échantillon
  instantané `MemorySample { t: i64 (epoch s), used_bytes: u64, total_bytes: u64 }`. Le `%` et le
  formatage Go sont calculés **côté webview** ; garde-fou `total_bytes == 0 → 0 %`. Helper pur
  `used_pct(used, total) -> f64` (0 si `total == 0`) testable.

### D3 — Persistance : **sampler continu dans le process tray + fichier JSONL borné**

**Décision : un thread de fond échantillonne la RAM en continu et append chaque point dans un
fichier JSONL persistant ; la webview n'échantillonne plus (elle lit l'historique).**

**D3a — Où brancher le sampler.** Nouveau module `src-tauri/src/memory.rs` exposant
`start_sampler(app: AppHandle)`, lancé depuis `setup()` (`lib.rs`) **exactement comme**
`mqtt_sub::start` : un `std::thread::spawn` détaché. Boucle : lire un échantillon → append disque →
`app.emit("tray://memory", sample)` → `sleep(cadence)`. Un premier échantillon est pris
**immédiatement** au démarrage (point rapide après lancement). Le thread **ne panique jamais** (toute
erreur I/O est loggée `eprintln!` et la boucle continue). Détaché → meurt avec le process (aucun
handling `Exit` requis, cf. `lib.rs:66-74`).

**D3b — Cadence.** **1 échantillon / 60 s** (plus lâche que le live 2 s de la v1 précédente, puisqu'on
stocke). Constante `SAMPLE_INTERVAL_SECS = 60`.

**D3c — Format & emplacement.** Fichier **JSONL append-only** :
`app.path().app_data_dir()?/memory-history.jsonl` (dir créé au besoin). Une ligne par échantillon :
`{"t":<epoch_s>,"u":<used_bytes>,"tot":<total_bytes>}` (clés courtes pour limiter la taille ;
sérialisées via `serde_json`). Append = robuste, lisible, sans base ni dépendance nouvelle.

**D3d — Rétention bornée = pas de downsampling en MVP.** Rétention **24 h glissantes**. À 60 s → **1440
points max** : fichier minuscule **et** nombre de points directement rendable en une polyligne SVG,
**sans agrégation**. On borne donc la rétention **pour éviter le downsampling** (cf. point 3 du
besoin). **Compaction** (troncature de la fenêtre) : réécriture atomique (écrire
`memory-history.jsonl.tmp` filtré sur `t >= now - 86400`, puis `rename` par-dessus), déclenchée **au
démarrage du sampler** puis **périodiquement** (toutes les `COMPACT_EVERY = 60` écritures ≈ 1 h). Le
fichier reste ainsi borné même sur des jours d'exécution continue.

**Concurrence.** Le thread sampler **écrit**, la commande `get_memory_history` **lit** : sérialiser
les accès par un `Mutex` porté par `AppState` (ex. `memory: Mutex<MemoryLog>` où `MemoryLog` tient le
`PathBuf` résolu). Les I/O sont encapsulées dans des **fonctions pures** prenant le chemin en
paramètre (testables sur un dossier temporaire) : `append_sample(path, &MemorySample)`,
`read_history(path) -> Vec<MemorySample>` (triée par `t` croissant, lignes corrompues ignorées),
`compact(path, retention_secs, now)`.

**Alternative rejetée — ring-buffer binaire à taille fixe (records + tête).** Plus compact mais
illisible, non introspectable, et surdimensionné pour 1440 lignes. JSONL + compaction périodique est
plus simple, réutilise `serde_json`, et reste trivial à déboguer. MVP.

**Alternative rejetée — persistance via le daemon / `iatc-core` / MQTT.** La RAM est une métrique du
**poste hôte de la GUI**, orthogonale au contrat conso ; l'y injecter alourdirait le contrat et le
daemon pour aucun bénéfice. Le sampler reste **local au process tray**.

### D4 — Commandes & événement (contrat backend ↔ webview)

- **`get_memory_history` (commande)** : lit le fichier persistant et renvoie la série de la fenêtre de
  rétention, **triée par `t` croissant**. Params : *aucun* (la rétention borne déjà la taille ;
  paramètre `since` = hors scope). Retour Rust `Vec<MemorySample>` (serde `rename_all = "camelCase"`)
  → TS `MemorySample[]`.
- **Événement `tray://memory`** : à chaque échantillon, le sampler émet le dernier `MemorySample`. La
  fenêtre de détails **écoute** cet événement pour faire **croître la courbe en direct sans polling**
  (cohérent avec `tray://state`).
- **`get_memory_sample` (live volatile) : SUPPRIMÉ du périmètre.** La v1 le proposait ; il devient
  inutile — la fenêtre charge l'historique persistant à l'ouverture (`get_memory_history`) puis
  s'appuie sur l'événement `tray://memory` pour le point courant. Zéro buffer volatil, zéro
  `setInterval` de polling → la vue reste **event-driven** comme le reste de l'app.

| | Backend Rust | Webview TS (`src/types.ts`) |
|---|---|---|
| Type point | `MemorySample { t: i64, used_bytes: u64, total_bytes: u64 }` (serde camelCase) | `interface MemorySample { t: number; usedBytes: number; totalBytes: number }` |
| Commande | `#[tauri::command] get_memory_history() -> Vec<MemorySample>` | `invoke<MemorySample[]>("get_memory_history")` |
| Événement | `app.emit("tray://memory", &sample)` | `listen<MemorySample>("tray://memory", …)` |

### D5 — Rendu : line chart SVG « maison », alimenté par l'historique persistant

- Fonction **pure** `memoryChart(samples: MemorySample[]): HTMLElement` dans `src/history.ts`,
  réutilisant `svg()` / `el()` / `emptyState()` et les classes `viz-*` (aucune lib de charting).
- **Line chart** : polyligne du % dans le temps (jusqu'à ~1440 points), axe Y 0–100 % (repères
  0/50/100), axe X = temps sur la fenêtre de rétention (labels `hh:mm`), quadrillage léger façon
  `viz-grid`, readout courant + tooltips `<title>`. Empty-state « Aucun échantillon mémoire pour
  l'instant » si `< 2` points.
- Câblage dans `analytics.ts` : à l'ouverture, `invoke("get_memory_history")` → buffer local (copie
  de travail, non persistée) → `memoryChart`. `listen("tray://memory")` → push + trim d'affichage
  (24 h) + re-render. `unlisten` sur `beforeunload`. Le bouton « Rafraîchir » recharge aussi
  l'historique mémoire.

---

## Étapes d'implémentation (commits atomiques)

1. **Toolchain & dépendance** (pas de code applicatif) : `rustc --version` ≥ **1.95** ? Ajouter à
   `src-tauri/Cargo.toml` `sysinfo = { version = "0.39", default-features = false, features =
   ["system"] }` (ajuster selon la vérif + doc 0.39.3). `cargo build` OK. Commit :
   `chore(tray): ajout dependance sysinfo (memoire systeme)`.

2. **Module `memory.rs` — cœur persistant testable** (`src-tauri/src/memory.rs`) :
   - `struct MemorySample { t, used_bytes, total_bytes }` (serde camelCase) + `used_pct(used, total)`.
   - Lecture système : `read_sample() -> MemorySample` (sysinfo, refresh mémoire seule).
   - Fonctions pures I/O prenant le chemin : `append_sample(path, &MemorySample)`,
     `read_history(path) -> Vec<MemorySample>` (triée `t` asc, lignes illisibles ignorées),
     `compact(path, retention_secs, now)` (réécriture atomique tmp+rename).
   - Tests unitaires (dossier temp) : `used_pct(0,0)==0.0`, `used_pct(8,16)==50.0` ; round-trip
     append→read conserve les points triés ; `compact` retire les points plus vieux que la rétention
     et garde les récents ; `read_history` d'un fichier absent → `Vec` vide (défensif, pas d'erreur) ;
     une ligne corrompue est ignorée sans planter. Commit : `feat(tray): coeur persistance memoire (JSONL borne)`.

3. **Sampler + commande + état** (`memory.rs`, `state.rs`, `lib.rs`) :
   - `AppState` : champ `memory: Mutex<MemoryLog>` (chemin résolu via `app_data_dir`), initialisé au
     `setup` (créer le dir).
   - `start_sampler(app)` : thread détaché (patron `mqtt_sub::start`) — sample immédiat puis boucle
     60 s : `append` + `emit("tray://memory")` + compaction périodique. Ne panique jamais.
   - `#[tauri::command] get_memory_history()` : verrouille, `read_history`, renvoie la série.
   - `lib.rs` : lancer `memory::start_sampler` dans `setup()` (après `mqtt_sub::start`) + ajouter
     `memory::get_memory_history` à `generate_handler!`.
   - Commit : `feat(tray): sampler RAM continu + commande get_memory_history`.

4. **Types webview** (`src/types.ts`) : `interface MemorySample { t; usedBytes; totalBytes }`.
   Commit : `feat(webview): type MemorySample`.

5. **Rendu SVG** (`src/history.ts`) : `memoryChart(samples): HTMLElement` (pure) — line chart % 0–100,
   readout, tooltips, empty-state. Commit : `feat(webview): line chart SVG memoire systeme`.

6. **Câblage vue de détails** (`analytics.html`, `src/analytics.ts`) :
   - `analytics.html` : `<section class="an-memory"><h2 class="viz-title">Mémoire système
     (RAM)</h2><div id="memory"></div></section>`.
   - `analytics.ts` : charger `get_memory_history` à l'ouverture + sur « Rafraîchir » ;
     `listen("tray://memory")` → append + trim 24 h + re-render ; `unlisten` sur `beforeunload`.
     Défensif : une erreur d'invoke/listen n'écrase pas la vue.
   - Commit : `feat(webview): historique RAM persistant dans la fenetre de details`.

7. **Styles minimaux** (`src/styles.css`) : readout + section, veine `viz-*`. Commit :
   `style(webview): habillage section memoire`.

---

## Fichiers concernés

- `src-tauri/Cargo.toml` — ajout `sysinfo` (étape 1).
- `src-tauri/src/memory.rs` — **nouveau** : `MemorySample`, `used_pct`, `read_sample`,
  `append_sample`/`read_history`/`compact`, `start_sampler`, `get_memory_history` (étapes 2-3).
- `src-tauri/src/state.rs` — `AppState.memory: Mutex<MemoryLog>` (étape 3).
- `src-tauri/src/lib.rs` — `mod memory;`, lancement du sampler dans `setup()`, enregistrement de la
  commande (étape 3).
- `src/types.ts` — `MemorySample` (étape 4).
- `src/history.ts` — `memoryChart()` SVG pur (étape 5).
- `src/analytics.ts` — chargement historique + écoute `tray://memory` + cleanup (étape 6).
- `analytics.html` — section `#memory` (étape 6).
- `src/styles.css` — habillage minimal (étape 7).

---

## Comportement attendu (critères d'acceptation testables)

- **[build]** `npm run build` et `cargo build` OK sur l'OS courant.
- **[Rust unit]** `cargo test` vert : `used_pct`, round-trip append/read trié, `compact` (rétention),
  fichier absent → vide, ligne corrompue ignorée.
- **[typecheck]** `npm run typecheck` vert (`MemorySample`, invoke/listen typés).
- **[lint]** `npm run lint` vert.
- **[réel, observable]** À l'ouverture de la fenêtre de détails, la section « Mémoire système (RAM) »
  affiche une **courbe** (axe Y 0–100 %) + readout `% · Go` ; un nouveau point apparaît **~toutes les
  60 s** en direct (via `tray://memory`), sans action utilisateur.
- **[réel, PERSISTANCE — fermeture/réouverture]** Laisser le tray tourner quelques minutes,
  **fermer** la fenêtre de détails, la **rouvrir** : la courbe **réaffiche l'historique déjà
  accumulé** (elle ne repart pas de zéro).
- **[réel, PERSISTANCE — redémarrage du tray]** **Quitter puis relancer** l'app tray, rouvrir la
  fenêtre : les points **antérieurs au redémarrage** (dans la fenêtre de rétention) sont **toujours
  présents** (fichier `memory-history.jsonl` relu).
- **[réel, borne disque]** Après une longue exécution, `memory-history.jsonl` reste borné (≈ ≤ 1440
  lignes ; points plus vieux que 24 h absents).
- **[réel, multi-OS]** Fichier créé au bon emplacement et courbe vivante sur **macOS** et sur
  **Windows/Linux** (au moins un 2ᵉ OS si dispo ; sinon reste-à-tester noté dans l'état des lieux).

## Vérification

- [ ] Toolchain vérifiée vs MSRV `sysinfo` ; version figée.
- [ ] `cargo build` + `cargo test` verts (tests persistance).
- [ ] `npm run build` / `typecheck` / `lint` verts.
- [ ] Test réel : courbe vivante ; **survit à fermeture/réouverture** ET à **redémarrage du tray**.
- [ ] `memory-history.jsonl` borné (compaction effective).
- [ ] Aucune régression sur quota / historique tokens / popover / icône de tray.
- [ ] Commits atomiques par étape.

## Hors scope (périmètre fermé)

- **Autres métriques** : CPU, disque, réseau, swap, températures, **per-process** — RAM système seule.
- **Downsampling / agrégation** de la série (évité par la rétention bornée 24 h) ; rétention longue
  (7 j+) et sa stratégie d'agrégation.
- **Rétention / cadence configurables** (constantes : 60 s, 24 h, 1440 pts).
- **Ring-buffer binaire** / base de données ; persistance via daemon / `iatc-core` / **MQTT**.
- **Publication MQTT** de la mémoire, intégration au contrat conso.
- **Alerting / seuils** sur la RAM, notifications.
- **Purge/rotation multi-fichiers**, export CSV, sélecteur de plage temporelle dans l'UI.
- **Corrélation** RAM ↔ conso de tokens (widget purement système).
