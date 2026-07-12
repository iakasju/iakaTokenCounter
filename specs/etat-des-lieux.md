# État des lieux — 2026-07-12

## En une phrase
La feature **visibilité conso/quota Codex** (leviers A + B) est **implémentée et validée
qualité** (Legolas PASS : 101 tests verts, clippy 0 warning, typecheck vert) ; un **nouveau
bundle `.app` est buildé et lancé** depuis `target/release/bundle/macos/` — en attente du
**verdict visuel du décideur** (seul gate non automatisable) avant install `/Applications`.
7 commits d'avance sur `origin/main` (non poussés).

## Fait récemment
- **Diagnostic Codex** (reprise du point d'attention « quota Codex fenêtre 30j non mappée ») :
  le parsing/mesure Codex fonctionne (`scan_codex` sort de vrais tokens sur les rollouts réels) ;
  la cause du « rien ne s'affiche » = sur le **plan free**, la fenêtre de rate-limit vaut
  `43200 min` (30 j), non mappée sur 5h/7j → aucun réservoir → carte Codex vide, et l'accès
  analytics (double-clic) étant branché sur la carte de quota, tout Codex devenait invisible.
- **Cadrage Gandalf** — instruction fermée `specs/instructions/feature-codex-visibilite-quota.md`
  (leviers A + B). Fact-check : `merge()` fabrique déjà une carte `codex/default` (branche 4) mais
  **vide** → A est un correctif de **rendu**, pas de plomberie MQTT.
- **Implémentation Gimli** (5 commits, A puis B) :
  - **A** — `src/render.ts` : carte conso-only (jauge rendue ssi `remainingPct` non null,
    ligne de conso `used_tokens` lisible, empty-state honnête, double-clic analytics conservé).
  - **B** — fenêtre `Window::ThirtyDay` (code `30d`, **hors `all()`** → cohérence Claude 5h/7j) ;
    `codex_window(40320..=44640) → ThirtyDay` ; `codex_reservoirs` free → réservoir 30d
    Official/CodexRollout ; contrat MQTT étendu (additif, non-breaking) ; `state.rs` + `icon.rs`
    (worst + icône) + `types.ts`/`render.ts` (`thirtyD`, jauge `30j`).
- **Gate qualité Legolas — PASS** : `cargo test --workspace` 101 passed / 0 failed ;
  `cargo clippy -D warnings` 0 warning ; `npm run typecheck` vert ; `npm run build` OK.
  Écarts du dev vérifiés légitimes (`icon.rs` requis par critère B ; tests de mapping mis à jour).
- **Build + bundle** : sidecars release régénérés (`iakahub`, `iakatc-daemon`, 12/07 22:08) +
  `.app` buildé et **lancé** (3 process frais up). Le `.dmg` échoue (`bundle_dmg.sh`) — optionnel,
  non bloquant (distribution par `.app`).

## En cours
- **Test réel en attente** : nouveau bundle lancé depuis `target/release/bundle/macos/iakaTokenCounter.app` ;
  le décideur doit valider de visu la carte Codex (conso + jauge `30j`), le double-clic → analytics,
  et la non-régression Claude. Branche `main` propre.

## Jalons (gates)
| Jalon | Statut |
|---|---|
| Instruction cadrée | oui (`feature-codex-visibilite-quota.md`, A+B) |
| Tests verts | oui (101 : core + daemon + iakahub + tray ; clippy + typecheck verts) |
| Recette stage | **attendue** (build lancé ; validation visuelle humaine en attente) |
| Feu vert prod | non applicable (produit local ; pas de squad Helm engagé) |

## Prochaine étape
**Recueillir le verdict visuel de Stéphane** sur le bundle en cours d'exécution (carte Codex
conso + jauge `30j`, double-clic analytics, jauges Claude 5h/7j intactes). Si OK : installer le
bundle dans `/Applications` (remplacement de l'ancien) puis checkpoint « update iakaframe ».

## Points d'attention
- **Gate humain non automatisable** : le levier A (rendu TS) n'a **aucun test auto** (pas de
  vitest dans le projet) → sa validation dépend entièrement du test à l'écran. Legolas l'a acté.
- **Push distant à confirmer** : 7 commits en avance sur `origin/main`, non poussés. Le blocage
  token Forgejo (401 `write:repository`) des reprises précédentes est **à re-vérifier** avant push.
- **Dette légère tracée par Legolas** (non bloquante) : tests de bornes manquants pour
  `formatTokens` (999/1000/1_000_000) et `codex_window` aux limites `40320`/`44640` ;
  ligne conso désormais possible aussi sur carte Claude (homogénéité assumée, à confirmer à l'œil).
- **Plan Codex payant** : les fenêtres pertinentes (5h primaire + 7j hebdo) **mappent déjà** ;
  aucun code à changer si passage en payant. Le `30d` ne concerne que le plan free.
- **`.dmg`** : `bundle_dmg.sh` échoue (volume/hdiutil ou automatisation Finder) — distribution
  par `.app` en attendant ; CI multi-OS + notarisation toujours non faits.

## Journal de reprise
- **2026-07-12** — Feature **visibilité conso/quota Codex (A+B)**. Diagnostic : plan Codex **free**
  → fenêtre 30 j (`43200 min`) non mappée sur 5h/7j → carte vide + analytics inatteignable (double-clic
  branché sur la carte de quota). Cadrage Gandalf (A = rendu conso-only ; B = `Window::ThirtyDay`),
  implémentation Gimli (5 commits), gate **Legolas PASS** (101 tests, clippy 0, typecheck vert).
  Build `.app` + sidecars release régénérés, bundle **lancé** pour test réel. Prochaine reprise :
  verdict visuel de Stéphane → install `/Applications` + push (re-vérifier le token Forgejo).
- **2026-07-09** — Correctif macOS « app menubar pure » : `ActivationPolicy::Accessory` (runtime)
  d'abord, jugée **insuffisante** (icône Dock réapparaissait) ; corrigé pour de bon via
  `LSUIElement=true` dans `src-tauri/Info.plist` (agent statique). Icône Dock supprimée + tuile
  « récente » résiduelle retirée à la main → **confirmé OK par le décideur**. Barres Claude vides
  au démarrage = latence de première capture (pas un bug). Push Forgejo **toujours bloqué**
  (token 401, `.git/config` + `$FORGEJO_TOKEN`). Prochaine reprise : régénérer un token
  `write:repository` et pousser les 41 commits.
- **2026-07-08** — Jalon MVP+iakahub livré et validé en recette réelle (5 gates PASS, 94 tests,
  36 commits locaux non poussés). Prochaine reprise : pousser sur Forgejo (token à régénérer),
  puis choisir une suite (bridge iakabox / vendoring Cockpit / tokenizer).
