# Instruction : Refonte visuelle du tray (icône réservoirs + popover barres)

> Rédigé par la coordination (P1). Consommé par Gimli. Fait suite au feedback décideur en
> recette live : « la jauge est ronde et une seule lecture ». Deux surfaces à refaire.

---

## Contexte

La GUI tray livrée affiche une jauge **ronde, une seule lecture** — insuffisant. Le décideur
a validé (via maquettes Loki) une refonte de **deux surfaces distinctes** :

1. **L'IHM détaillée** (popover) → **hypothèse 1 : barres horizontales** (réservoirs qui se vident).
2. **L'icône de la barre de menus** (tray icon) → **logo officiel de l'IA + 2 mini-réservoirs**
   (5h + 7j), repli **1 barre** si l'IA n'a qu'un quota.

Spécifications visuelles de référence (à respecter) :
- `docs/design/tray-icon-spec.html` — **spec pixel de l'icône** (cotes, palette hex, mapping,
  états, repli, **SVG officiels inline** des logos, section « Spec Gimli »).
- `docs/design/popover-reservoir-hypotheses.html` — l'**hypothèse 1** pour le popover.

## Ce qui existe

| Élément | Où | État |
|---|---|---|
| Rendu popover (jauge ronde) | `src/render.ts` (`gauge()`), `src/styles.css` | à remplacer par barres |
| Icône tray simple + tooltip | `src-tauri/src/tray.rs` | à remplacer par icône composée |
| État réservoirs (par compte, 5h/7j, confidence/source) | `src-tauri/src/state.rs` (`get_reservoirs`) | **réutilisé tel quel** |
| Spec design | `docs/design/*.html` | validé décideur |

## Décision

### D1 — Popover : barres horizontales (hyp.1)
Une **carte par compte** ; pour chaque fenêtre (5h, 7j) une **barre horizontale** dont la
largeur = **% restant**, teintée par le niveau, avec le **compte à rebours** (`resets_at`) et
le **badge de confiance** (official / official_stale / local_estimate `~` / none `?`). Repli
**1 barre** si le compte n'a qu'une fenêtre de quota. Pas de dessin rond.

### D2 — Icône tray : logo officiel + 2 mini-réservoirs
Composer une icône **couleur (non-template)** selon `docs/design/tray-icon-spec.html` :
- **Canvas 40 × 18 px (@2x 80 × 36)** ; zone logo **16×16** à (1,1) ; deux pistes-réservoir
  **18×5** (pilule r 2,5) aux origines (20,3) et (20,10) ; repli 1 barre centrée à (20, 6.5).
- **Logo officiel de l'IA** à gauche (SVG inline fournis dans la spec : Claude sunburst
  `#D97757`, OpenAI nœud teal `#10A37F`, **fallback** pastille neutre `#6F6F78` + initiale).
- **Palette barres** : piste vide `#8E8E93`, ok `#34C759` (≥50 %), moyen `#FF9F0A` (20–49 %),
  alerte `#FF3B30` (<20 %). **Mapping** `w = max(2, round(pct/100 × 18))`.
- **Incertitude sans mentir** : estimé → hachure 45° ; inconnu → piste pointillée (jamais de
  faux plein).
- **Rasterisation RGBA** puis `tray.set_icon()` à chaque mise à jour d'état (seule la largeur
  des barres change ; le logo peut être pré-rasterisé/caché). Techniquement : composer le SVG
  puis rasteriser (`resvg`) OU dessiner les rects en `tiny-skia` avec logo en cache. Gimli
  tranche selon la stack déjà présente et **épingle la dépendance**.
- **Lisibilité clair ET sombre** : icône couleur non-template (ne s'inverse pas) — vérifier le
  contraste sur les deux fonds (halo/contour 0,5 px seulement si nécessaire).

### D3 — Multi-comptes (comportement de l'icône)
L'icône ne montre qu'**un** compte à la fois. Au MVP : afficher le compte **le plus critique**
(plus petit % restant, toutes fenêtres confondues) ; le **popover liste tous** les comptes.
Le **tooltip** reste le pire réservoir. (Aujourd'hui un seul compte à quota — claude/max — donc
comportement trivial, mais coder la sélection « pire compte ».)

## Étapes d'implémentation

1. **Popover (front)** : dans `src/render.ts`, remplacer `gauge()` par un rendu **barres
   horizontales** (SVG/CSS) selon hyp.1 ; adapter `src/styles.css`. Réutiliser l'état
   `get_reservoirs` inchangé. Gérer les 4 états de confiance + repli 1 barre + countdown.
2. **Assets logos** : extraire les **SVG officiels inline** de `docs/design/tray-icon-spec.html`
   vers des assets réutilisables (Rust) + le fallback générique. Table provider → logo.
3. **Icône tray (Rust)** : nouveau module de **composition + rasterisation RGBA** de l'icône
   40×18 (logo + 2 barres) selon la spec (cotes/palette/mapping/incertitude/repli).
4. **Câblage** : dans `src-tauri/src/tray.rs`, appeler `set_icon()` avec l'icône composée à
   chaque mise à jour d'état ; sélection du **pire compte** (D3).
5. **Tests** : mapping `% → largeur` (bornes 0/ <20 / 20-49 / ≥50 / 100), sélection du pire
   compte, repli 1 barre, choix de teinte par niveau, choix du traitement par confiance.

## Fichiers concernés

- `src/render.ts`, `src/styles.css` — popover barres (D1).
- `src-tauri/src/tray.rs` — set_icon dynamique + sélection pire compte (D2/D3).
- `src-tauri/src/` (nouveau module, ex. `icon.rs`) — composition/rasterisation RGBA.
- assets logos (SVG inline → module Rust ou fichiers embarqués).
- `src-tauri/Cargo.toml` — dépendance de rasterisation (resvg/tiny-skia), épinglée.

## Comportement attendu

- Le **popover** affiche, par compte, **2 barres** (5h/7j) largeur = % restant, teintées par
  niveau, avec countdown + badge de confiance ; **1 barre** si un seul quota. Plus aucune
  jauge ronde.
- L'**icône de la barre de menus** montre **le logo officiel** de l'IA la plus critique + ses
  **2 mini-réservoirs** (ou 1 en repli), teintés par niveau, alerte en rouge <20 %.
- **Incertitude** : barre estimée hachurée (`~`), inconnue pointillée (`?`) — jamais de faux plein.
- L'icône reste **lisible sur menubar clair et sombre**.
- Les valeurs suivent l'état MQTT retenu (mêmes données que `get_reservoirs`), rafraîchies au tick.
- Les **50+ tests** existants restent verts ; nouveaux tests sur mapping/sélection/repli.

## Vérification

- [ ] Typecheck + lint (front) OK
- [ ] `cargo check` + clippy `-D warnings` OK
- [ ] Tests ajoutés (mapping %→largeur, pire compte, repli, teinte, confiance) verts ; aucun régressé
- [ ] Rebuild bundle macOS + testé en réel par le décideur (icône + popover, clair/sombre)

## Hors scope

- La **fenêtre analytics** (inchangée).
- Nouveaux **providers** au-delà de ce que l'état expose déjà (claude/codex + fallback générique).
- **Bundling CI** Windows/Linux, notarisation.
- Toute modif de la **logique de mesure/publication** du daemon ou d'**iakahub**.
