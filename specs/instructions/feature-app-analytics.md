# Instruction : App analytics (fenêtre double-clic — historique par compte)

> Rédigé par Gandalf (P1 — cadrage). Consommé par Gimli comme instruction de travail.
> Troisième brique : la **vue d'historique** qui **remplit le hook** `open_analytics(...)` laissé
> en stub « À venir » par l'instruction tray. **Même app Tauri, nouvelle vue** — pas une app séparée.

---

## Contexte

Le moniteur tray (instruction frère `feature-tray-jauges.md`) affiche l'**état courant** (jauges de
quota par compte, lues en MQTT retained). Il prévoit un **hook** : un **double-clic sur une carte de
réservoir** appelle une commande Tauri `open_analytics(...)` qui, au MVP tray, ouvre une **fenêtre
stub « À venir »**. **Cette instruction remplit ce stub** : une vue d'**historique de consommation**
pour le compte double-cliqué — courbes tokens/jour, ventilation coordinateur/sous-agent, par projet
et par provider, avec le **quota courant** (5h/7d) du compte en tête.

Le point dur, déjà anticipé par le Cap (`PROJET.md`) : **il n'y a pas d'historique dans le
transport**. Le daemon ne publie que `current`/`last` **retained** (pas de série temporelle), et la
**persistance CouchDB est hors scope** (elle toucherait iakaboxlogs). Il faut donc **fermer la source
de l'historique** (D2).

### Faits vérifiés / acquis (veille Gandalf, sources en bas)

- **`economy.rs` (Rust, testé)** calcule DÉJÀ l'historique **tokens/jour/projet** *all-time* depuis
  les JSONL Claude Code : `fold_activity_line`/`scan_projects_activity` → `Vec<ProjectActivity>`
  (jours triés, rayon ∝ tokens/jour), et le **coût par projet** avec split **coordinateur vs
  sous-agent** (`scan_projects_dir` → `Vec<ProjectEconomy>` : `input/output/coord/sub`). Ces
  fonctions sont **pures, publiques, lecture seule**. Elles seront **portées dans `iatc-core`** par
  l'instruction daemon (module `measure/claude.rs`).
- **Codex** : `iatc-core/measure/codex.rs` (instruction daemon) compte les tokens des rollouts, mais
  n'a pas (encore) de ventilation **par jour** équivalente à `fold_activity_line`.
- **Composants de visualisation d'IakaCockpit** (référence/inspiration, à ré-adapter — pas vendorés) :
  `EconomyPanel.tsx` (sparkline tokens de sortie/tour + totaux + split coord/sous-agent),
  `ActivityTimeline.tsx` (scatter-timeline : 1 ligne = 1 projet, bulle = 1 jour, rayon ∝ tokens/jour),
  `TreemapPanel.tsx` (treemap des tokens par projet). Tous **présentationnels purs**, SVG en JSX,
  aucune dépendance de charting, aucun I/O.
- **Quota courant** : déjà disponible dans le backend de l'app tray (état MQTT retained, commande
  `get_reservoirs()` de l'instruction tray) — l'analytics le **réutilise**, il ne le recalcule pas.

## Ce qui existe

| Élément | Où | État |
|---|---|---|
| Hook `open_analytics(...)` (stub « À venir ») | app tray (`feature-tray-jauges.md`) | à **remplir** ici |
| Activité tokens/jour + coût/projet (Claude) | `iatc-core` (porté d'`economy.rs`) | fonctions **prêtes**, read-only |
| Comptage Codex | `iatc-core/measure/codex.rs` | présent ; **pas** de ventilation par jour |
| État quota courant (MQTT retained) | app tray, `get_reservoirs()` | prêt, réutilisable |
| Composants viz (réf.) | `IakaCockpit/src/components/{EconomyPanel,ActivityTimeline,TreemapPanel}.tsx` | référence à ré-adapter |
| Vue analytics | — | **absente (objet de cette instruction)** |

## Décision

### D1 — L'analytics est une **vue de l'app tray**, pas une app séparée

**Retenu** : une **fenêtre Tauri `analytics`** (ou une route dans la même webview), ouverte par la
commande `open_analytics(provider, account)` qui **remplace le stub**. Même binaire, même backend,
même client MQTT. La fenêtre reçoit en paramètre l'**identité de la carte** double-cliquée.

**Pourquoi** : le décideur l'impose (« même app Tauri, nouvelle vue ») ; cela réutilise le backend
existant (état MQTT + accès `iatc-core`) sans dupliquer de process.

> **Micro-choix tranché** : la signature devient `open_analytics(provider, account)` (l'instruction
> tray avait esquissé `open_analytics(account)`) — l'historique se filtre par **provider**, il faut
> donc le porter. Raffinement mineur, compatible : la carte tray connaît déjà `(provider, account)`.

### D2 — Source de l'historique : **relecture disque via `iatc-core`** (option a), aucun store nouveau

**Retenu** : à l'ouverture de la vue, le **backend Rust** appelle les fonctions **read-only** de
`iatc-core` qui **re-scannent les logs locaux** (JSONL Claude / rollouts Codex) et renvoient
l'activité *all-time* (tokens/jour/projet) et le coût par projet (coord/sub). **Pas de nouveau store,
pas de persistance.**

**Pourquoi (a) et pas (b) un store SQLite qui accumulerait le live MQTT** :
- **Réutilise l'existant** : `economy.rs` calcule déjà exactement ces séries — zéro logique nouvelle
  de calcul.
- **Zéro persistance nouvelle** : cohérent avec « ne pas toucher iakaboxlogs » et « CouchDB hors
  scope ». Un SQLite maison serait un **second magasin** à gérer (schéma, migration, rétention) pour
  une donnée **déjà sur le disque** dans les JSONL — sur-ingénierie au MVP.
- **Historique complet immédiat** : (b) ne connaîtrait que l'historique *depuis son installation* (il
  accumule le live) ; (a) voit **tout le passé** présent dans les JSONL dès le premier lancement.

**Écarté** : *(b) store SQLite live-accumulé* → repoussé (utile seulement le jour où l'on voudra une
granularité infra-journalière ou une source qui s'efface du disque ; ce n'est pas le cas des JSONL).

> **Conséquence** : la vue est **recalculée à l'ouverture** (et sur rafraîchissement manuel), pas
> temps-réel. Acceptable : un historique n'a pas besoin d'être live (le live, ce sont les jauges du
> tray). **Micro-choix tranché** : rafraîchissement **à l'ouverture + bouton « Rafraîchir »**, pas de
> polling continu.

### D3 — Extension **read-only** de `iatc-core` (seule modif tolérée du cœur)

Le daemon (`iatc-core`/`iatc-daemon`) est **figé** sauf pour **exposer des lectures d'historique**.
Périmètre **strict** de cette extension :
- **Claude** : rendre publiques / réutiliser telles quelles `scan_projects_activity` (tokens/jour/
  projet) et `scan_projects_dir` (coût/projet coord/sub). Rien à écrire.
- **Codex** : **ajouter une fonction read-only de ventilation par jour** (`scan_codex_activity`),
  **miroir** de `fold_activity_line`, lisant les `token_count` des rollouts et bucketant par jour/
  projet. C'est la **seule addition** — pure, testable, sans toucher la mesure ni la publication.
- **Commande Tauri** (dans le backend de l'app, PAS dans le daemon) : `get_history(provider)` qui
  appelle ces fonctions et renvoie `{ activity: ProjectActivity[], economy: ProjectEconomy[] }`.

**Pourquoi cette borne** : garder le daemon intact (contrat de mesure/publication inchangé) tout en
donnant à la GUI un accès **lecture** au même calcul. L'ajout Codex par jour est calqué sur du code
Claude déjà testé → risque minimal.

### D4 — Granularité honnête : historique **par provider**, quota courant **par account**

Les JSONL/rollouts **ne portent pas** l'étiquette de compte (`account` est une étiquette manuelle du
canal statusline — cf. contrat, drapeau `account_ambiguous`). **Conséquence assumée** :
- **L'historique** (courbes/treemap) est ventilé par **projet** et **coord/sub**, à l'échelle du
  **provider** double-cliqué (Claude *ou* Codex), **pas** filtré par `account`.
- **Le quota courant** en tête de vue (jauges 5h/7d) est **bien celui du compte** `(provider,
  account)`, lu depuis l'état MQTT retained (`get_reservoirs()`).
- La vue **affiche clairement** cette portée (« Historique Claude Code — tous comptes de ce provider
  sur ce poste ») pour ne pas laisser croire à un filtrage par compte qui n'existe pas.

> **Micro-choix tranché** : ne pas inventer d'attribution compte↔JSONL. On montre la vérité
> mesurable (par provider) + le quota exact du compte en tête. C'est la même honnêteté que le
> `confidence`/`account_ambiguous` du contrat.

### D5 — Visualisations : **ré-adaptées** des composants Cockpit (référence, pas vendoring)

**Retenu** : trois vues, **ré-implémentées** dans cette app en s'**inspirant** des composants Cockpit
(présentationnels purs, SVG/JSX, sans lib de charting), adaptées à **un seul provider** :
1. **Timeline tokens/jour** (réf. `ActivityTimeline.tsx`) : 1 ligne = 1 projet, bulle = 1 jour, rayon
   ∝ tokens/jour.
2. **Treemap par projet** (réf. `TreemapPanel.tsx`) : surface ∝ tokens totaux du projet.
3. **Split coordinateur / sous-agent** (réf. `EconomyPanel.tsx`) : totaux `input/output` + part
   `coord` vs `sub` (donnée honnête via `sidechain`).

**Pourquoi ré-adapter et pas importer** : le **vendoring IakaCockpit est hors scope** (et Cockpit
dépend de son i18n/hooks). On **recopie l'idée** (peu de code, SVG pur) en la branchant sur les types
`iatc-core` (`ProjectActivity`, `ProjectEconomy`).

> **Micro-choix tranché** : viz **maison SVG** (pas de dépendance de charting ajoutée), fidèles aux
> composants Cockpit mais autonomes.

## Étapes d'implémentation

1. **Fenêtre/route `analytics`** dans l'app Tauri (déclaration `tauri.conf.json` ou routeur front) ;
   `open_analytics(provider, account)` **remplace le stub** : ouvre la vue avec ces paramètres.
2. **Extension read-only `iatc-core`** (D3) : confirmer `pub` sur `scan_projects_activity` /
   `scan_projects_dir` (Claude) ; **ajouter `scan_codex_activity`** (tokens/jour/projet Codex, miroir
   testé de `fold_activity_line`). Aucune modif de la mesure/publication du daemon.
3. **Commande Tauri `get_history(provider)`** (backend app) : appelle `iatc-core` selon le provider →
   `{ activity, economy }`. Défensive (dossier de logs absent → séries vides, pas d'erreur).
4. **En-tête quota** : réutiliser l'état MQTT (`get_reservoirs()`), filtrer sur `(provider, account)`,
   afficher les jauges 5h/7d + confiance (mêmes styles que le tray).
5. **Viz front** (D5) : composants `HistoryTimeline`, `HistoryTreemap`, `HistorySplit` (SVG purs)
   branchés sur `get_history` ; empty-state honnête si aucune donnée.
6. **Bandeau de portée** (D4) : libellé explicite « par provider, tous comptes » + rappel de la
   limitation `account_ambiguous`.
7. **Rafraîchissement** : chargement à l'ouverture + **bouton « Rafraîchir »** (re-scan). Pas de
   polling (D2).
8. **Mocks/tests** : fixtures JSONL Claude (multi-projets, coord + sidechain, plusieurs jours) et
   rollouts Codex (avec `token_count`) → vérifier `scan_codex_activity` et le mapping vers les 3 viz.
9. **README** (section app) : ce que montre la vue, la source (relecture disque, all-time), la
   limitation par provider, le rafraîchissement manuel.

## Fichiers concernés

- `iatc-core/src/measure/codex.rs` — **ajout** `scan_codex_activity` (read-only, par jour). *(seule
  modif core tolérée)*
- `iatc-core/src/measure/claude.rs` — confirmer visibilité `pub` des fonctions d'activité/économie.
- `src-tauri/src/history.rs` — commande `get_history(provider)` (appelle `iatc-core`).
- `src-tauri/src/lib.rs` (ou `main.rs`) — `open_analytics(provider, account)` (remplace le stub) +
  enregistrement `get_history` ; déclaration fenêtre `analytics`.
- `src-tauri/tauri.conf.json` — fenêtre `analytics`.
- `src/…` (front) — vue analytics + `HistoryTimeline`/`HistoryTreemap`/`HistorySplit` + en-tête quota
  + bandeau de portée + bouton Rafraîchir.
- tests : `#[cfg(test)]` `scan_codex_activity` ; test front d'agrégation vers viz (fixtures).

## Comportement attendu

Critères **observables et testables** :

- Un **double-clic** sur la carte `(claude, max)` du tray **ouvre la fenêtre analytics** (le stub
  « À venir » a disparu) intitulée pour Claude Code.
- La vue affiche en **tête** les **jauges de quota 5h/7d du compte** `(claude, max)` avec leur badge
  de confiance (valeurs issues de l'état MQTT retained, identiques au tray).
- Avec des JSONL Claude sur ≥ 2 projets et ≥ 2 jours, la **timeline** montre **une ligne par projet**
  et **une bulle par jour**, rayon croissant avec les tokens du jour ; le **treemap** montre une
  tuile par projet (surface ∝ tokens totaux) ; le **split** affiche la part **coordinateur vs
  sous-agent**.
- Ouvrir l'analytics pour `(codex, default)` montre l'historique **Codex** (via `scan_codex_activity`)
  avec des séries **> 0** sur une fixture réelle de rollout.
- Sur un poste **sans aucun log** du provider, la vue s'ouvre avec un **empty-state honnête** (aucune
  bulle/tuile fantôme), **sans erreur**.
- Le **bandeau de portée** indique clairement « historique par provider, tous comptes » (limitation
  `account_ambiguous`).
- Le bouton **« Rafraîchir »** **re-scanne** le disque et **met à jour** les viz (ajouter un JSONL
  puis rafraîchir → la série change) ; **aucun polling** entre deux rafraîchissements.
- `get_history(provider)` renvoie `{ activity, economy }` cohérents avec `iatc-core` sur des fixtures
  (test unitaire), et **des séries vides** si le dossier de logs est absent (pas d'exception).
- `scan_codex_activity` bucket correctement tokens/jour/projet et **ignore** les lignes non
  pertinentes (test unitaire, miroir des tests `fold_activity_line`).

## Vérification

- [ ] `cargo check` / typecheck front OK
- [ ] `cargo clippy` + lint front OK
- [ ] `cargo test` vert : `scan_codex_activity` (bucketing/jour, cas ignorés), `get_history` sur
      fixtures, mapping front → viz
- [ ] Build Tauri OK (OS de dev au minimum) ; fenêtre `analytics` s'ouvre
- [ ] Testé dans l'app réelle : double-clic carte → historique réel (Claude ET Codex), quota en tête,
      empty-state, bouton Rafraîchir après ajout d'un log

## Hors scope

- **Persistance CouchDB** / toute **modification d'iakaboxlogs**.
- **Toute modif de `iatc-core`/`iatc-daemon` AUTRE que** l'ajout read-only `scan_codex_activity` +
  l'exposition `pub` des fonctions d'activité (la mesure et la publication du daemon restent figées).
- **Store local SQLite** live-accumulé (repoussé, D2).
- **Attribution compte↔JSONL** (impossible sans ID de compte dans les logs — D4).
- **Vendoring IakaCockpit** (on s'inspire des composants, on ne les importe pas).
- **Providers autres que Claude Code + Codex** ; **export/reporting avancé** (CSV, PDF, partage) ;
  **temps réel** de l'historique (le live reste au tray).

---

## Sources (veille)

- `economy.rs` — activité tokens/jour (`scan_projects_activity`) + coût/projet coord/sub
  (`scan_projects_dir`) : `IakaCockpit/src-tauri/src/economy.rs`
- Composants viz de référence : `IakaCockpit/src/components/{EconomyPanel,ActivityTimeline,TreemapPanel}.tsx`
- Hook `open_analytics` (stub à remplir) : `specs/instructions/feature-tray-jauges.md`
- Codes quota courants réutilisés en tête de vue : `specs/contrat-mqtt-conso.md`
- Cap + décision « CouchDB hors scope, ne pas toucher iakaboxlogs » : `specs/PROJET.md`
