# État des lieux — 2026-07-09

## En une phrase
Le MVP d'iakaTokenCounter **et** le backbone local **iakahub** sont livrés, testés (94 tests
verts, 5 gates qualité PASS) et **validés en recette réelle** ; ajout ce jour d'un correctif
macOS **app menubar pure (plus d'icône Dock)**, ré-installé dans `/Applications` et vérifié en
direct. Tout est commité en local sur `main` (41 commits) mais **toujours non poussé** (token
Forgejo invalide — 401 confirmé côté API).

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
- **[2026-07-09] Correctif macOS « app menubar pure »** — deux temps :
  (1) `ActivationPolicy::Accessory` posée au runtime dans le setup Tauri (`src-tauri/src/lib.rs`) ;
  s'est révélée **insuffisante** : l'icône Dock **réapparaissait** (flash au lancement / création
  de fenêtre à l'exécution). (2) **Correctif solide** : `src-tauri/Info.plist` avec
  `LSUIElement=true` (fusionné par Tauri v2 → agent macOS **statique**, autoritaire dès le
  lancement, insensible aux fenêtres) ; l'appel runtime est conservé (couvre `tauri dev`).
  Résultat : **plus d'icône Dock ni d'entrée ⌘-Tab** ; tray + popover intacts (vérifié
  `LSUIElement=true` dans le bundle + `lsappinfo type="UIElement"`). Une **tuile « récente »
  résiduelle** du Dock (héritée des lancements *Regular* d'avant le fix) subsistait : retirée à la
  main (clic droit → Retirer du Dock) ; ne revient plus (un agent ne s'ajoute pas aux récentes).
  Build `.app` OK (le `.dmg` échoue sur `-1743` : automatisation Finder non autorisée — non
  bloquant, distribution par `.app`). Nouveau bundle ré-installé dans `/Applications`.
  Diagnostic « barres Claude vides » au démarrage = **latence de première capture** (pas de bug) :
  l'info officielle n'existe qu'une fois que la statusline Claude Code a émis `rate_limits` ; les
  barres se remplissent au tick suivant (daemon publie 312 codes, broker connecté).

## En cours
- Rien en développement actif. Branche `main` propre ; `doc/index.html` (page de présentation)
  désormais suivi et inclus dans ce checkpoint.

## Jalons (gates)
| Jalon | Statut |
|---|---|
| Instruction cadrée | oui (5 instructions : collecteur, tray, analytics, iakahub, tray-visuals) |
| Tests verts | oui (94 : 52 core + 4 daemon + 13 iakahub + 25 tray) |
| Recette stage | oui (installée + lancée + chaîne vérifiée + design validé décideur) |
| Feu vert prod | non applicable (produit local ; pas de squad Helm engagé) |

## Prochaine étape
Au choix du décideur (rien d'urgent) :
- **Push Forgejo** dès qu'un token valide est fourni (41 commits à archiver) — *le plus
  prioritaire pour la sauvegarde distante*. Le token actuel (`.git/config` **et** `$FORGEJO_TOKEN`)
  est refusé en 401 → **régénérer un token `write:repository`** sur Forgejo puis pousser.
- Ou enchaîner une suite de vision (voir Points d'attention).

## Points d'attention
- **Push distant bloqué** : token Forgejo invalide (401 confirmé sur `/api/v1/user`, en header
  comme en basic). Le dépôt `sjupin/iakaTokenCounter` existe et répond (200) ; le remote `origin`
  est câblé (ancien token périmé dans l'URL). Besoin d'un token valide (`write:repository`).
- **Correctif Dock — points de fragilité** : (a) la commande `statusLine` de `~/.claude/settings.json`
  pointe le binaire `/Applications/iakaTokenCounter.app/.../iakatc-daemon` (désormais le nouveau
  build ré-installé — OK) ; (b) l'étiquette de compte est figée à `max` dans cette commande ;
  (c) le `.dmg` requiert d'autoriser l'automatisation Finder (Réglages → Confidentialité →
  Automatisation) si l'on veut un installeur packagé.
- **Dette technique tracée** (non bloquante) : `.../last` retained repoussé ; pas de harnais
  de test front JS (logique portée en Rust) ; bundling CI Windows/Linux + `.dmg` + notarisation
  non faits ; SIGKILL sur iakahub ne déclenche pas la cascade (invariant OS documenté).
- **Suites de vision** (backlog) : bridge iakahub → Mosquitto iakabox (vue portefeuille) ;
  vendoring du daemon/broker dans IakaCockpit (widgets economy/log) ; brique tokenizer
  (fallback conso) ; quota Codex (fenêtre 30j non mappée sur 5h/7j).
- **Trademark** : logos officiels (Claude, OpenAI) utilisés en usage nominatif d'identification
  (note dans `src-tauri/README.md`).

## Journal de reprise
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
