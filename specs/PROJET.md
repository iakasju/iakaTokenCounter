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
| Collecteur de conso (parsing logs locaux) | `specs/instructions/feature-collecteur-logs.md` | à spécifier |
| Tray multi-OS + jauge quota/compte | `specs/instructions/feature-tray-jauges.md` | à spécifier |
| App locale d'analytics (double-clic) | `specs/instructions/feature-app-analytics.md` | à spécifier |
| Brique comptage tokens (fallback conso) | `specs/instructions/feature-tokenizer.md` | à spécifier |

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
