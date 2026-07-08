# État des lieux — 2026-07-08

## En une phrase
Le MVP d'iakaTokenCounter **et** le backbone local **iakahub** sont livrés, testés (94 tests
verts, 5 gates qualité PASS) et **validés en recette réelle** sur le poste : l'app tourne en
autonomie totale (broker MQTT embarqué, zéro dépendance externe), tout est commité en local
sur `main` (36 commits) mais **non poussé** (token Forgejo invalide).

## Fait récemment
- **Amorçage** structure iakaframe (specs/, CLAUDE.md, gate qualité) — branche `main`.
- **Contrat MQTT** code/value figé (`specs/contrat-mqtt-conso.md`).
- **Daemon de mesure v0** (`iakatc-core` + `iakatc-daemon`) : conso Claude Code (portage
  `economy.rs`) + Codex + capture quota statusline + publish MQTT retained — Legolas PASS.
- **App analytics** (fenêtre historique SVG, relecture disque via `iakatc-core`) — Legolas PASS.
- **iakahub v0** : broker MQTT `rumqttd` local embarqué (`127.0.0.1`) + orchestration/supervision
  du daemon — supprime la dépendance au Mosquitto iakabox authentifié — Legolas PASS.
- **Refonte visuelle tray** : popover en barres horizontales (hyp.1) + icône = logo officiel
  de l'IA + 2 mini-réservoirs (5h/7j), rasterisation RGBA via resvg — Legolas PASS.
- **Recette réelle** : app installée dans `/Applications`, lancée ; quota Max réel capturé,
  chaîne iakahub→daemon→tray confirmée vivante (broker `127.0.0.1`, topics retained frais).
  Design **validé de visu par le décideur**.

## En cours
- Rien en développement actif. Branche `main` propre (seul `doc/` non suivi, préexistant,
  hors périmètre — à confirmer/nettoyer).

## Jalons (gates)
| Jalon | Statut |
|---|---|
| Instruction cadrée | oui (5 instructions : collecteur, tray, analytics, iakahub, tray-visuals) |
| Tests verts | oui (94 : 52 core + 4 daemon + 13 iakahub + 25 tray) |
| Recette stage | oui (installée + lancée + chaîne vérifiée + design validé décideur) |
| Feu vert prod | non applicable (produit local ; pas de squad Helm engagé) |

## Prochaine étape
Au choix du décideur (rien d'urgent) :
- **Push Forgejo** dès qu'un token valide est fourni (36 commits à archiver) — *le plus prioritaire pour la sauvegarde distante*.
- Ou enchaîner une suite de vision (voir Points d'attention).

## Points d'attention
- **Push distant bloqué** : `FORGEJO_TOKEN` refusé en 401 (token perdu/révoqué). Le dépôt
  Forgejo `sjupin/iakaTokenCounter` existe ; le remote `origin` est câblé. Besoin d'un token
  valide (`write:repository`) pour pousser. Recommandation : régénérer et pousser bientôt
  (tout le travail n'existe qu'en local).
- **Dette technique tracée** (non bloquante) : `.../last` retained repoussé ; pas de harnais
  de test front JS (logique portée en Rust) ; bundling CI Windows/Linux + `.dmg` + notarisation
  non faits ; SIGKILL sur iakahub ne déclenche pas la cascade (invariant OS documenté).
- **Suites de vision** (backlog) : bridge iakahub → Mosquitto iakabox (vue portefeuille) ;
  vendoring du daemon/broker dans IakaCockpit (widgets economy/log) ; brique tokenizer
  (fallback conso) ; quota Codex (fenêtre 30j non mappée sur 5h/7j).
- **Ménage** : deux anciennes copies de `iakaTokenCounter.app` rangées dans le scratchpad de
  session (supprimables).
- **Trademark** : logos officiels (Claude, OpenAI) utilisés en usage nominatif d'identification
  (note dans `src-tauri/README.md`).

## Journal de reprise
- **2026-07-08** — Jalon MVP+iakahub livré et validé en recette réelle (5 gates PASS, 94 tests,
  36 commits locaux non poussés). Prochaine reprise : pousser sur Forgejo (token à régénérer),
  puis choisir une suite (bridge iakabox / vendoring Cockpit / tokenizer).
