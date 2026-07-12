# Instruction : Visibilité conso/quota Codex (leviers A + B)

> Rédigé par Gandalf (P1 — cadrage). Consommé par Gimli (Claude Code) comme instruction de
> travail. Doc en français, code/identifiants en anglais.
> Interface partagée impactée : `specs/contrat-mqtt-conso.md` (levier B, extension additive).

---

## Contexte

Codex est **mesuré** (le parsing des rollouts fonctionne : `scan_codex` produit des mesures
réelles — ex. `iakaFrameGUI`=26894, `codex-recette`=24808, `codex-probe`=12157 used_tokens) mais
reste **inexploitable pour l'utilisateur** dans le tray, surtout sur le **plan free**.

Deux causes distinctes, deux leviers :

- **Levier A — la carte conso est aveugle.** La seule surface d'un provider dans le tray est la
  **carte de réservoir**, et le double-clic → analytics y est branché (`render.ts:190`). Une carte
  n'affiche que le **quota** (`remaining_pct`). Or la conso mesurée (`used_tokens`) **n'est jamais
  rendue** par la barre (`gauge()` ne lit que `remainingPct`). Résultat : quand un provider est
  mesuré mais n'a pas de quota exploitable, la carte montre des jauges `?` vides — la conso Codex
  est invisible et la carte paraît cassée.
- **Levier B — la fenêtre 30 j du free est jetée.** Sur le plan free, le rate-limit porte
  `window_minutes=43200` (30 j) avec `used_percent`/`resets_at` (données disponibles dans
  `CodexRateLimit`), mais `merge::codex_window` ne mappe que 5h et 7d → `None` → **aucune vraie
  jauge Codex free**. Le plan payant (≈5h + ≈7j) mappe déjà et fonctionne tel quel.

### Fait vérifié qui recadre le levier A (à connaître avant de coder)

Contrairement à l'hypothèse « free → aucun réservoir → aucune carte », la fusion produit **déjà**
un réservoir pour Codex free, et le tray le reçoit :

- `merge::merge` ajoute le couple `("codex","default")` pour tout provider **mesuré sans fichier
  quota** (`merge.rs:213-217`), puis la **branche 4** (`merge.rs:181-193`) émet un `Reservoir`
  avec `used_tokens = Some(total codex)`, `remaining_pct = None`, `confidence = none`, sur **5h ET
  7d**.
- `contract::quota` publie donc `iakatokencounter/all/ia/codex/default/quota/{5h|7d}/used_tokens`
  (= total mesuré) et `.../remaining_pct = null`.
- Le tray souscrit `all/ia/+/+/quota/#` (`mqtt_sub.rs:33`) → `codex/default` **matche** →
  `parse_quota_topic` l'accepte (`state.rs:115`) → **une carte `codex/default` existe**, avec
  `usedTokens` renseigné mais deux fenêtres en `?` (car `remainingPct = null`).

**Conséquence de cadrage :** le levier A n'a **pas** besoin de nouveau topic/section conso ni de
nouvelle souscription — la donnée `used_tokens` **arrive déjà** sur le topic quota. A est un
**problème de rendu**, pas de plomberie MQTT. (Étape 0 confirme ce comportement en réel avant de
polir.)

---

## Ce qui existe

| Élément | Où | État |
|---|---|---|
| Mesure Codex (rollouts) | `iakatc-core/src/measure/codex.rs::scan_codex` | ✅ fonctionne |
| `CodexRateLimit` (used_percent, window_minutes, resets_at) | `iakatc-core/src/measure/codex.rs:48-53` | ✅ parsé, 43200 min inclus |
| Mapping fenêtre Codex | `iakatc-core/src/quota/merge.rs::codex_window` (l.236-242) | ⚠️ 5h/7d seuls ; 43200 → `None` |
| Réservoir Codex best-effort | `iakatc-core/src/quota/merge.rs::codex_reservoirs` (l.247-272) | ⚠️ ne produit rien pour le free |
| Enum `Window` (core) | `iakatc-core/src/quota/merge.rs:20-39` | 2 variantes (FiveHour, SevenDay) |
| Branche 4 (measured sans quota) → carte `used_tokens`/pct null | `merge.rs:181-193`, `merge.rs:213-217` | ✅ produit déjà `codex/default` |
| Publication quota décomposée | `iakatc-core/src/publish/contract.rs::quota` (l.87-118) | ✅ générique sur `window.code()` |
| Assemblage daemon (merge + codex_reservoirs) | `iakatc-daemon/src/main.rs:91-99` | ✅ branché |
| Souscription tray | `src-tauri/src/mqtt_sub.rs:33` (`all/ia/+/+/quota/#`) | ✅ couvre tout `{window}` via `#` |
| Enum `Window` (tray) + `from_code` | `src-tauri/src/state.rs:20-34` | ⚠️ 5h/7d seuls ; sinon `None` (topic droppé) |
| Décodage topic quota | `src-tauri/src/state.rs::parse_quota_topic` (l.115-130) | ✅ délègue à `Window::from_code` (pas de hardcode 5h/7d) |
| `ReservoirCard` (core tray) | `src-tauri/src/state.rs:72-79` | 2 fenêtres (`five_h`, `seven_d`) |
| `ReservoirCard` (TS) | `src/types.ts:27-32` | 2 fenêtres (`fiveH`, `sevenD`) |
| Rendu carte + double-clic analytics | `src/render.ts::card` (l.157-192), dblclick l.190 | ✅ ; `gauge()` ne montre que `remainingPct` |
| Contrat MQTT (vocabulaire `{window}`) | `specs/contrat-mqtt-conso.md` §2 | `{5h|7d}` figés dans la doc |
| Harnais de test TS | `package.json` | ❌ aucun (pas de vitest) → A vérifié par typecheck + test manuel |

---

## Décision

Périmètre séquencé : **A d'abord** (débloque la visibilité, zéro risque MQTT), **puis B** (ajoute
la vraie jauge free). Les deux convergent vers **une seule règle de rendu** unifiée.

### Levier A — rendre la conso, sans toucher au MQTT

**Décision : A est un correctif de rendu pur (tray webview). Aucune modif du contrat MQTT, de la
souscription, ni du core Rust.**

- Rejeté : « ajouter une section/topic conso consommable par le tray + agréger l'axe conso dans
  `state.rs` ». Raison : le `used_tokens` par `(provider, account)` **arrive déjà** sur le topic
  quota (branche 4). Ajouter une 2ᵉ souscription (`all/ia/agents/#`) + un 2ᵉ chemin d'agrégation
  serait de la plomberie redondante pour une donnée déjà présente — contraire à MVP / réutiliser
  l'existant. (L'axe `ia/agents/.../conso` reste destiné aux consommateurs par-provider comme le
  Cockpit, pas à la carte par-compte du tray.)

**Règle de rendu unifiée (le cœur de A) :** dans `render.ts::card`,
1. une **barre de jauge** (`gauge()`) n'est rendue **que si la fenêtre a un `remainingPct` non
   null** (= une vraie jauge de quota) ;
2. la carte affiche **une ligne de conso** (`used_tokens` formaté, ex. « 26.9k tokens ») dès qu'au
   moins une fenêtre porte `usedTokens` ;
3. si **aucune** fenêtre n'a de jauge, la carte montre la ligne de conso + un **empty-state
   honnête** (« pas de jauge de quota disponible »), et **conserve le double-clic → analytics**.

Effets :
- **Codex free (A seul, avant B)** : carte `codex/default` = conso lisible + « pas de jauge » au
  lieu de deux `?` trompeurs. Analytics atteignable.
- **Claude avec statusline** : 5h/7d ont un `remainingPct` → jauges rendues comme aujourd'hui
  (aucune régression).
- **Claude mesuré sans statusline** (branche 4) : passe de deux `?` à une ligne de conso — même
  amélioration, comportement volontairement homogène entre providers.

### Levier B — modéliser la fenêtre 30 j (Codex free), extension additive

**Décision : ajouter une fenêtre `ThirtyDay` (code `30d`) alimentée UNIQUEMENT par le chemin
Codex (`codex_reservoirs`), sans l'injecter dans `Window::all()`.**

- Rejeté : ajouter `ThirtyDay` à `Window::all()`. Raison : `all()` est itéré par `merge()` pour
  **tout** `(provider, account)` → Claude hériterait d'une fenêtre `30d` vide (branche 4, pct null)
  → topics `claude/.../quota/30d/...` parasites et jauge fantôme. On préserve la cohérence Claude
  (5h/7d) en gardant `all()` = `[FiveHour, SevenDay]` ; `30d` n'est émis **que** quand un rate-limit
  Codex le renseigne.
- **Confiance/source** : `used_percent` provient du provider lui-même (rollout Codex), de même
  nature que le mapping payant 5h/7d existant → `Confidence::Official` + `Source::CodexRollout`
  (cohérent avec `codex_reservoirs` actuel). `resets_at` depuis le rate-limit, `captured_at =
  None`. La péremption reste gérée côté tray (`t` du payload + `now > resets_at`). Limite MVP
  assumée : pas de `official_stale` sur le 30d (le rollout peut être ancien) — noté hors-scope.
- **Tolérance de fenêtre** : `codex_window(40320..=44640) → ThirtyDay` (28–31 j ; 43200 = 30×1440
  au centre). Aucun chevauchement avec les bandes existantes (240–360, 8640–11520).

**Impact contrat MQTT (à surveiller — voir aussi § Compat) :** extension **additive** du
vocabulaire `{window}` de `{5h,7d}` à `{5h,7d,30d}`. Le **format de payload `{v,t}` est inchangé**,
la souscription `.../quota/#` couvre déjà `30d`, les anciens subscribers l'ignorent, un ancien
daemon ne l'émet jamais → **non-breaking**. À documenter dans le contrat comme extension additive
(note v1.1), **pas** une rupture v2.

**Point de compat critique côté consommateur :** `state.rs::Window::from_code` est le **seul**
endroit du tray qui fige l'ensemble des fenêtres (retourne `None` sinon → topic silencieusement
ignoré). Il **doit** apprendre `30d` sous peine de jeter la nouvelle donnée. `parse_quota_topic`
délègue à `from_code` → aucune autre modif de décodage.

### Interaction A × B (voulue)

Sur Codex free, après B : la carte reçoit `30d` (jauge réelle) **et** les `5h/7d` branche-4
(pct null, `used_tokens`). La règle de rendu de A résout la cohabitation sans cas particulier :
seul `30d` a un `remainingPct` → **une seule jauge (30d) rendue**, plus la ligne de conso. Les
`5h/7d` sans pct ne produisent pas de barres parasites.

---

## Étapes d'implémentation (commits atomiques)

### Bloc A — visibilité (priorité)

1. **(Étape 0 — vérif réelle, pas de code)** Lancer le daemon + tray sur l'environnement réel et
   confirmer qu'une carte `codex / default` apparaît déjà avec `usedTokens` non null et deux
   fenêtres `?`. Consigner le constat (si la carte est **absente**, investiguer d'abord les
   suspects : `used_tokens_by_provider(codex) > 0` au tick, `IAKATC_ACCOUNT_LABEL`, build daemon
   à jour — avant de continuer). Commit : néant (note dans l'état des lieux).

2. **Règle de rendu conso-only** (`src/render.ts`, `src/types.ts` si besoin) :
   - Ajouter un helper pur `shouldRenderGauge(w: WindowState): boolean` = `w.remainingPct !== null`.
   - Ajouter un helper pur `cardUsedTokens(r: ReservoirCard): number | null` (le `usedTokens` porté
     par n'importe quelle fenêtre — ils sont égaux pour la branche 4 ; prendre le max non-null).
   - Ajouter un helper pur `formatTokens(n: number): string` (ex. `26894 → "26.9k"`).
   - Réécrire `card()` : rendre `gauge()` seulement pour les fenêtres `shouldRenderGauge` ; ajouter
     une ligne de conso si `cardUsedTokens !== null` ; empty-state « pas de jauge de quota
     disponible » si aucune jauge ; conserver `dblclick → onOpenAnalytics`. Ajuster le libellé de
     tier (nombre de jauges, ou « conso seule »).
   - CSS minimal pour la ligne conso + l'empty-state (`src/` styles existants).
   - Commit : `feat(tray): carte conso-only pour provider mesure sans quota (Codex visible)`.

### Bloc B — jauge free (après A)

3. **Core : fenêtre `ThirtyDay`** (`iakatc-core/src/quota/merge.rs`) :
   - Ajouter la variante `Window::ThirtyDay` ; `code()` → `"30d"` ; **ne pas** l'ajouter à `all()`.
   - `codex_window` : `40320..=44640 => Some(Window::ThirtyDay)`.
   - Tests unitaires : `codex_window(43200) == Some(ThirtyDay)` ; `codex_reservoirs` sur un
     rate-limit free (used_percent, window_minutes=43200, resets_at) produit **1** réservoir `30d`,
     `source = CodexRollout`, `confidence = Official`, `used_pct`/`remaining_pct` renseignés ;
     non-régression `codex_window(300)`/`(10080)` et un test garantissant que `merge()` d'un provider
     mesuré ne produit **que** 5h/7d (pas de 30d parasite pour Claude).
   - Commit : `feat(core): fenetre 30d (Codex free) via codex_window/codex_reservoirs`.

4. **Contrat MQTT** (`specs/contrat-mqtt-conso.md`) :
   - Documenter `{window}` = `{5h|7d|30d}` (extension additive, note v1.1) ; ajouter une ligne
     d'exemple de topic `.../quota/30d/...` ; préciser que le payload et la souscription sont
     inchangés. Commit : `docs(contrat): fenetre 30d additive (Codex free)`.

5. **Tray core : 3ᵉ fenêtre** (`src-tauri/src/state.rs`) :
   - `Window::ThirtyDay` + `from_code("30d")`.
   - `ReservoirCard` et `Card` : champ `thirty_d: WindowState` (serde camelCase `thirtyD`).
   - `apply_message` : router `ThirtyDay`. `worst()` : inclure `30d`.
   - Tests unitaires : un topic `.../codex/default/quota/30d/remaining_pct/current` remplit
     `thirtyD` ; les tests existants (5h/7d) restent verts ; `worst()` peut être porté par `30d`.
   - Commit : `feat(tray): 3e fenetre 30d dans l'etat des reservoirs`.

6. **Tray types + rendu 30d** (`src/types.ts`, `src/render.ts`) :
   - `ReservoirCard` TS : `thirtyD: WindowState`.
   - `card()` : ajouter `30d` à la liste des fenêtres candidates (via `hasWindow` + la règle
     `shouldRenderGauge` de l'étape 2) ; libellé « 30j » ; `FRESHNESS_30D = 86400` (constante
     tray-side, pas de config daemon — MVP). `countdown()` gère déjà les jours.
   - Commit : `feat(tray): jauge 30j pour Codex free`.

---

## Fichiers concernés

- `src/render.ts` — règle de rendu conso-only + jauge 30j (A et B6).
- `src/types.ts` — `thirtyD` sur `ReservoirCard` (B6).
- `iakatc-core/src/quota/merge.rs` — `Window::ThirtyDay`, `codex_window`, tests (B3).
- `specs/contrat-mqtt-conso.md` — vocabulaire `{window}` additif `30d` (B4).
- `src-tauri/src/state.rs` — `Window::ThirtyDay`, `from_code`, `thirty_d`, `worst`, tests (B5).
- (lecture/validation seulement, aucune modif attendue : `iakatc-daemon/src/main.rs` — l'assemblage
  `merge + codex_reservoirs` propage `30d` automatiquement ; `publish/contract.rs::quota` est
  générique sur `window.code()` ; `mqtt_sub.rs` couvre `30d` via `#`.)

---

## Comportement attendu (critères d'acceptation testables)

### Levier A
- **[tray, manuel]** Un provider mesuré sans quota exploitable (Codex free) affiche une carte avec
  sa **conso lisible** (`used_tokens` formaté) et **aucune** jauge `?` trompeuse ; l'empty-state
  « pas de jauge de quota disponible » est présent.
- **[tray, manuel]** Le **double-clic** sur cette carte ouvre bien l'analytics du provider.
- **[tray, manuel]** Une carte Claude avec statusline rend toujours ses jauges 5h/7d à l'identique
  (aucune régression).
- **[typecheck]** `npm run typecheck` vert (helpers purs typés).

### Levier B
- **[Rust unit]** `codex_window(43200) == Some(Window::ThirtyDay)` ; `codex_window(300)` et
  `(10080)` inchangés ; `codex_window(500) == None`.
- **[Rust unit]** `codex_reservoirs` sur un rate-limit free produit exactement **1** réservoir
  `window == ThirtyDay`, `source == CodexRollout`, `confidence == Official`, `used_pct`/
  `remaining_pct` cohérents (`remaining = 100 - used`), `resets_at` propagé.
- **[Rust unit]** `merge()` pour un provider **mesuré sans fichier quota** ne produit **que** des
  réservoirs `FiveHour`/`SevenDay` (jamais `ThirtyDay`) → cohérence Claude préservée.
- **[Rust unit, tray]** `state::Window::from_code("30d") == Some(ThirtyDay)` ; un topic
  `.../quota/30d/remaining_pct/current` alimente `card.thirtyD` ; les tests 5h/7d existants restent
  verts.
- **[tray, manuel]** Codex free affiche **une** jauge `30j` réelle (pct + compte à rebours) + la
  ligne de conso ; pas de barres 5h/7d parasites. La jauge `30d` alimente aussi l'icône de tray et
  le `worst`.

---

## Vérification

- [ ] `cargo test` (core + tray) vert, tests B ajoutés.
- [ ] `npm run typecheck` vert.
- [ ] Test réel : carte Codex free lisible (conso + jauge 30j), double-clic analytics OK, Claude
      sans régression.
- [ ] Contrat MQTT à jour (`30d` documenté additif).
- [ ] Commits atomiques par étape.

---

## Compat MQTT — points à surveiller

1. **Nouveau `{window}` = `30d`** : extension **additive** du vocabulaire de topics. Payload `{v,t}`
   inchangé ; souscription `.../quota/#` couvre déjà `30d` ; anciens subscribers l'ignorent ; ancien
   daemon ne l'émet pas. → **non-breaking**. Documenter en note v1.1 (pas de bump v2).
2. **`state.rs::Window::from_code`** = unique verrou consommateur sur l'ensemble des fenêtres : doit
   apprendre `30d` sinon la donnée est silencieusement jetée. `parse_quota_topic` délègue → rien
   d'autre à changer côté décodage.
3. **`ReservoirCard` gagne `thirtyD`** (Rust + TS) : contrat de rendu backend↔webview (camelCase
   `thirtyD`) — les deux côtés doivent bouger ensemble.
4. **Levier A n'introduit AUCUN topic ni souscription** : il consomme le `used_tokens` déjà publié
   sur `.../quota/{5h|7d}/used_tokens` (branche 4). Ne pas ajouter de plomberie conso pour A.

---

## Hors scope

- Persistance des métriques (déjà hors scope contrat, § 0).
- Multi-comptes Codex (les logs ne portent pas d'`account` → `default` seul, D4).
- Refonte de la vue analytics elle-même (le hook `open_analytics` existe déjà ; A garantit juste
  l'accès).
- Ajout d'une fenêtre 30j pour Claude (Claude reste 5h/7d).
- `official_stale` sur le 30d / seuil de fraîcheur dédié en config daemon (MVP : `Official` +
  `FRESHNESS_30D` tray-side constant).
- Capture statusline Codex / plafonds configurés Codex.
- Traitement fin de `secondary` payant au-delà du mapping 7d existant (fonctionne déjà).
