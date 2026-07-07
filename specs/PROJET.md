# PROJET — iakaTokenCounter

> Espace de réflexion (cadrage). Document de vision et de specs.
> **Aucun code n'est écrit ici** — seulement des décisions et des spécifications.

---

## Vision

**Un moniteur de consommation IA multi-comptes, en icône de barre système (tray).**

Sur un poste de travail qui consomme de l'IA de **plusieurs sources / comptes** (Claude,
OpenAI, etc.), iakaTokenCounter affiche dans le **tray** un **petit réservoir (jauge) par
compte IA** — on voit d'un coup d'œil, compte par compte, **ce qu'il reste du quota du
plan** (le contingent se recharge périodiquement). Un **double-clic sur les barres** ouvre
une **app locale de logs & analytics** (historique, courbes, détail par compte).

L'app est **autonome** : elle tourne **sans dépendance au projet iaka** (utile sur
n'importe quel poste). Mais **sa pertinence explose dans le cadre iakaproject** (corrélation
avec les projets, les agents et la chaîne).

> Note d'histoire : le nom vient d'un cadrage initial « compteur de tokens ». Le comptage
> de tokens reste une **brique interne** (estimer une conso quand la source ne la donne
> pas), mais **le produit est le moniteur tray + analytics**.

## Cap (nord) — iakalogs central de coms, iakaTokenCounter daemon de mesure

> Architecture pub/sub. **iakaboxlogs (iakalogs) est promu « central de coms »** (hub MQTT
> + CouchDB déjà déployés) — pas de nouveau broker. iakaTokenCounter est un **producteur**.

**Rôles :**
- **iakalogs = backbone** : broker MQTT (`192.168.2.11:1883` / `9883` WS) + store CouchDB.
  Point de passage unique ; à terme absorbe logs + routage des conversations vers l'extérieur.
- **iakaTokenCounter = daemon de mesure** (headless) : mesure **conso / limits / quota** de
  tokens et **publie dans iakalogs**, selon deux axes d'agrégation :
  - `.../all/projets/agents/...` — par **projet × agent**,
  - `.../all/ia/agents/...` — par **IA (fournisseur) × agent**.
- **Subscribers** : **IakaCockpit** (widgets `economy`/`log`) et la future **GUI tray**
  d'iakaTokenCounter s'abonnent à ce qui les intéresse — ils ne recalculent pas.

**Principe d'allègement client — messages `retained` :**
- Le daemon publie l'**état courant** et le **dernier** (`current` / `last`) en **MQTT
  retained**, pour que tout client lise l'état directement (pas de rejeu des logs bruts).

**Cœur de mesure** : réutilise `IakaCockpit/src-tauri/src/economy.rs` (Rust, testé) pour
l'usage Claude Code, étendu à **Codex** + **capture quota** (statusline `rate_limits`).

> **Contrainte (décideur) : on ne touche PAS au dépôt iakaboxlogs** (garde son nom, son
> code). Le daemon **publie sur son broker Mosquitto existant** (un broker accepte tout
> topic sans modif). La **persistance CouchDB** des métriques (qui toucherait le pont
> iakaboxlogs) est **repoussée** ; le MVP se contente des messages MQTT `retained`.

## Objectifs

- Afficher dans le tray une **jauge de quota restant par compte IA** (≥ 1 source réelle).
- Alimenter les jauges depuis les **logs locaux des agents** (aucune clé API requise pour le MVP).
- Ouvrir sur **double-clic** une **app locale d'analytics** (historique de conso par compte).
- Fonctionner en **multi-OS** (macOS + Windows, Linux souhaité).
- Rester **runnable en standalone**, sans rien du projet iaka.

## Périmètre

**Dans le scope :**
- Tray icon multi-OS avec une jauge « quota du plan restant » par compte.
- Collecte de conso par **parsing des logs locaux d'agents** (source(s) à confirmer par Gandalf).
- App locale d'analytics ouverte au double-clic (logs, courbes, agrégats par compte/période).
- Modèle de « réservoir » = **quota du plan qui se recharge** (% restant sur la fenêtre du plan).

**Hors scope (pour l'instant) :**
- Interrogation des APIs usage/billing des fournisseurs (clés API) — itération ultérieure possible.
- Comptage de texte « playground » (coller un prompt → nombre de tokens) comme produit principal.
- Multimodal / image.

---

## Stack technique — décision

| Couche | Choix | Raison |
|---|---|---|
| Tray + app locale | **Tauri v2** (Rust + webview) | Multi-OS, léger (<10 Mo, ~35 Mo RAM) pour un outil toujours ouvert, local-first, front TS |
| Langage | TypeScript (front) + Rust (backend Tauri minimal) | Cohérent iakaframe, tray natif cross-OS |
| Parsing usage | **ccusage** (MIT, TS) importé comme lib | Parse déjà les JSONL Claude Code **+ Codex** ; on ne réécrit pas ça |
| Capture quota | canal **statusline** `rate_limits` persisté en fichier + estimation JSONL en repli | Quota exact Pro/Max quand dispo, estimé étiqueté sinon (mode hybride) |
| Brique comptage | lib TS interne (fallback conso quand le log ne donne pas l'usage) | — |
| Données locales | store local (SQLite/JSON) | Self-hosted, offline, analytics |

> Rappel méthode : self-hosted/open-source d'abord ; cloud en fallback justifié.

---

## Sources de données / dépendances externes

| Besoin | Source | Quota / coût | Stratégie en dev |
|---|---|---|---|
| Conso par compte | logs locaux des agents (Claude Code JSONL, Cursor, Codex…) | gratuit (local) | à confirmer / mock dans `specs/mock/` |
| Quota du plan (recharge) | à déterminer (log ? config manuelle ?) | — | veille Gandalf |
| Comptage tokens (fallback) | lib TS locale | gratuit | — |

---

## Architecture (esquisse, à confirmer par cadrage)

```
  logs locaux agents ──► collecteur/parseur ──► store local (SQLite/JSON)
   (Claude Code, …)                                   │
                                                      ├─► TRAY: 1 jauge « quota restant » / compte
                                                      │
                                              double-clic
                                                      │
                                                      └─► APP LOCALE analytics (historique, courbes)
```

---

## Backlog des features

Chaque feature reçoit son fichier dans `specs/instructions/` AVANT implémentation.

| Feature | Instruction | État |
|---|---|---|
| Daemon de mesure v0 (conso + quota, publish MQTT) | `specs/instructions/feature-collecteur-logs.md` | **livré v0 — Legolas PASS** (local, non poussé) |
| Tray multi-OS + jauge quota/compte | `specs/instructions/feature-tray-jauges.md` | **livré v0.2 — Legolas PASS** (mergé main, non poussé) |
| App locale d'analytics (double-clic) | `specs/instructions/feature-app-analytics.md` | **cadré, validation en attente** |
| Brique comptage tokens (fallback conso) | `specs/instructions/feature-tokenizer.md` | à spécifier |

> Contrat partagé : `specs/contrat-mqtt-conso.md` (topics code/value, retained current/last).

---

## Décisions structurantes (journal)

> Trace courte des arbitrages importants — le « pourquoi » qui se perd sinon.

- **2026-07-07** — Création du projet, 1er cadrage « compteur de tokens » (CLI+lib).
- **2026-07-07** — **Recadrage majeur (décideur)** : le produit est un **moniteur de
  consommation IA multi-comptes en tray**, avec **jauge de quota (plan qui se recharge)
  par compte**, **double-clic → app locale d'analytics**, **runnable en standalone** mais
  à pertinence maximale dans iakaproject. Le comptage de tokens devient une brique interne.
- **2026-07-07** — **Arbitrages posés** : conso alimentée par **parsing des logs locaux
  d'agents** (pas d'API clé pour le MVP) ; réservoir = **quota du plan (se recharge)** ;
  cible **multi-OS dès le départ**.
- **2026-07-07** — **Veille technique Gandalf** : le quota exact (`rate_limits`,
  fenêtre 5h + hebdo) n'existe **que** sur le canal statusline de Claude Code (Pro/Max),
  pas dans les JSONL (qui sous-comptent). ccusage (MIT/TS) parse déjà Claude Code + Codex.
- **2026-07-07** — **4 décisions verrouillées (décideur)** :
  1. **Quota = mode hybride** : statusline exact quand dispo, estimation JSONL+plafond
     sinon, avec **niveau de confiance affiché** par jauge.
  2. **Réutiliser ccusage** comme lib TS pour le parsing usage (Claude Code + Codex).
  3. **MVP = Claude Code + Codex CLI** (les deux sources à JSONL locaux exploitables).
     Cursor/Gemini/Copilot repoussés (OAuth/cookies).
  4. **Techno = Tauri v2** (tray multi-OS + fenêtre analytics).
  Défauts raisonnables ajustables plus tard : icône tray simple + jauges dans la
  popover (pas de dessin fin dans l'icône au MVP) ; afficher fenêtre 5h ET hebdo.
- **2026-07-07** — **Recon écosystème (Gandalf)** : le collecteur JSONL existe déjà, testé,
  en Rust/Tauri dans `IakaCockpit/src-tauri/src/economy.rs` (porté de `naonedge-dashboard/
  scan.js`) ; iakaboxlogs fournit déjà Mosquitto + CouchDB. Personne ne capte le quota.
- **2026-07-07** — **Décision d'architecture (décideur)** : iakaTokenCounter devient le
  **berceau d'IakaDaemon**. On **déplace le collecteur utile depuis IakaCockpit** vers ici
  (partie daemon). Le **cœur d'IakaDaemon = un bridge/relais MQTT** ; iakaTokenCounter y
  **publie** conso/quota (topics `iakatokencounter/<projet>`, `ALL/claude/current`,
  `.../<plan>/available`…). Le **code du daemon est vendored dans IakaCockpit** (widgets
  `economy`/`log` le consomment) et **consommé par la GUI** d'iakaTokenCounter. Le daemon
  est en **Rust** (réutilise economy.rs, copie Rust→Rust) → **ccusage abandonné** pour CC.
  À terme : fusion des logs + routage conversations vers l'extérieur. IakaDaemon **bridge**
  vers le Mosquitto central d'iakaboxlogs (pas de nouveau broker).
- **2026-07-07** — **⚠️ Discipline MVP à trancher** : le cap est vaste ; définir la 1re
  bouchée livrable (probable : daemon local qui publie tokencounter/quota en MQTT + tray
  qui souscrit ; logs-fusion et routage externe = phases ultérieures).
- **2026-07-07** — **Précision d'archi (décideur) — remplace l'idée « IakaDaemon relais »** :
  pas de nouveau backbone. **iakalogs est promu « central de coms »** (le hub MQTT existant).
  **iakaTokenCounter = daemon de mesure** qui publie conso/limits/quota **dans iakalogs**,
  axes `.../all/projets/agents/...` et `.../all/ia/agents/...`. **IakaCockpit s'abonne** à ce
  qui l'intéresse. **Messages `retained` (`current`/`last`)** pour alléger les calculs clients.
  → Impacte aussi le dépôt **iakaboxlogs** (schéma de topics + rôle central). À remonter à Odin.
- **2026-07-07** — **Contrainte (décideur)** : **ne pas toucher au dépôt iakaboxlogs** pour
  l'instant (nom + code inchangés). Le daemon publie sur son **broker Mosquitto existant**
  (aucune modif broker requise pour ajouter des topics). Persistance CouchDB des métriques
  **repoussée** ; MVP = publication MQTT `retained` seule. Feu vert cadrage daemon v0.
