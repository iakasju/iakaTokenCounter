# iakaTokenCounter

**Un moniteur de consommation IA multi-comptes, dans la barre système.**

Sur un poste qui consomme de l'IA depuis **plusieurs comptes** (Claude, OpenAI…),
iakaTokenCounter affiche dans le tray une **jauge de quota restant par compte** — on voit
d'un coup d'œil ce qu'il reste du contingent de chaque plan, sachant qu'il se recharge
périodiquement. Les données viennent du **parsing des logs locaux des agents**, pas d'une
API distante.

Un **double-clic** sur les jauges ouvre une **application locale d'analytics** :
historique, courbes, détail par compte.

L'application est **autonome** — elle tourne sans aucune dépendance à l'écosystème iaka,
même si c'est là qu'elle prend tout son sens. Le comptage de tokens est une **brique
interne** (mécanisme de repli pour estimer la consommation), pas le produit.

---

## Installation

La version scellée courante est **[v0.1.0](../../releases/tag/v0.1.0)** — voir
[toutes les versions](../../releases).

> **À ce stade, les releases publient les sources, pas de binaire pré-compilé.**
> L'application se construit depuis l'archive de la version.

**Prérequis :** Node.js ≥ 20, Rust stable (avec `cargo`), et les
[dépendances système de Tauri 2](https://v2.tauri.app/start/prerequisites/) pour votre
plateforme (Xcode CLT sur macOS, WebView2 + Build Tools sur Windows, `webkit2gtk` et
`libayatana-appindicator` sur Linux).

```bash
# 1. Récupérer l'archive de la version depuis la page des releases
#    (Assets > Source code), puis la décompresser
cd iakaTokenCounter-0.1.0

# 2. Installer les dépendances front
npm ci

# 3. Lancer en développement
npm run tauri dev

# 4. Ou produire l'exécutable de votre plateforme
npm run tauri build
```

Le binaire est produit dans `src-tauri/target/release/bundle/`. Au lancement,
l'application se loge dans la barre système ; elle n'a pas de fenêtre principale.

---

## Stack

| Composant | Rôle |
|---|---|
| `src/` | Interface d'analytics — TypeScript · Vite |
| `src-tauri/` | Coquille de bureau et icône de tray — Tauri 2 · Rust |
| `iakatc-core/` | Cœur du modèle : comptes, quotas, agrégation |
| `iakatc-daemon/` | Collecte : surveillance et parsing des logs locaux |
| `iakahub/` | Connecteur optionnel vers le hub iaka |

L'ensemble Rust est un **workspace Cargo** (`Cargo.toml` racine), sous licence MIT.

## Développement

```bash
npm run dev         # interface d'analytics seule (Vite)
npm run tauri dev   # application complète, avec le tray
npm run typecheck   # vérification des types
npm run build       # build du front

cargo test          # tests du workspace Rust
cargo clippy        # lint Rust
```

## Documentation

- [`specs/PROJET.md`](./specs/PROJET.md) — vision et spécifications.
- [`specs/instructions/`](./specs/instructions/) — les instructions de travail, une par lot.
- [`CLAUDE.md`](./CLAUDE.md) — contrat de travail des agents sur ce dépôt.

## Méthode

Ce projet est développé selon la méthode
[**iakaframe**](https://github.com/iakasju/iakaframe) : un décideur au-dessus d'une équipe
de rôles à périmètres étanches, et une instruction écrite et validée avant toute ligne de
code.
