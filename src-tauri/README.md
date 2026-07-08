# iakaTokenCounter — GUI tray (Tauri v2)

La **face visible** du produit : une **icone de barre systeme** qui affiche, dans une popover,
une **carte de reservoir par compte IA** avec deux jauges (fenetre **5 h** et **7 j**) et un
**badge de confiance**. C'est un **pur subscriber** du contrat MQTT retained : elle **lit** les
codes publies par le daemon et les **rend** — elle ne recalcule rien et ne touche jamais MQTT
depuis la webview.

> Interface d'entree figee par [`../specs/contrat-mqtt-conso.md`](../specs/contrat-mqtt-conso.md).
> Cadrage : [`../specs/instructions/feature-tray-jauges.md`](../specs/instructions/feature-tray-jauges.md).

## Architecture

```
  iakahub (sidecar)  ── broker MQTT local 127.0.0.1 (rumqttd, anonyme)
      │  spawne + supervise
      ▼
  iakatc-daemon (enfant d'iakahub)  ──publish retained──►  broker local iakahub
                                                              │  subscribe (retained, QoS 1)
                                                              ▼
   backend Rust (rumqttc, thread)  ──etat en memoire──►  ReservoirCard[]
        │  evenement tray://state  +  commande get_reservoirs
        ▼
   webview (popover TS)  ──rend les cartes / jauges / badges de confiance
```

> **Backbone local (iakahub)** : la GUI ne spawne plus le daemon directement — elle spawne
> **`iakahub`**, qui embarque un **broker MQTT local** (`127.0.0.1`, anonyme) **et** lance/supervise
> le measure daemon a cote de lui. Le poste est **standalone** : aucune dependance a un broker
> externe. Cadrage : [`../specs/instructions/feature-iakahub.md`](../specs/instructions/feature-iakahub.md).

- **Backend Rust** (`src/mqtt_sub.rs`, `src/state.rs`) : seul a parler MQTT. Abonnements
  `…/all/ia/+/+/quota/#` (jauges, decouverte dynamique des comptes) et `…/meta/daemon/#`.
- **Tray** (`src/tray.rs`) : icone simple + tooltip du **pire reservoir**, clic gauche =
  popover, menu droit = Ouvrir / Quitter.
- **Sidecar** (`src/lib.rs`) : le backbone **`iakahub`** est **spawne** au demarrage (voir plus bas).
- **Vue analytics** (`src/analytics.rs`, `src/history.rs`) : double-clic sur une carte ->
  fenetre d'**historique** du provider (voir plus bas).

## Vue analytics — historique (feature-app-analytics)

Un **double-clic** sur une carte de reservoir ouvre une **fenetre analytics** `(provider, account)`
(meme app Tauri, meme backend). Elle montre :

- **En tete** : le **quota courant 5h/7d du compte** double-clique (memes jauges/badges que le tray,
  lues depuis l'etat MQTT retained via `get_reservoirs`).
- **Corps** : l'**historique de consommation**, en **trois visualisations SVG maison** (aucune lib
  de charting) re-adaptees d'IakaCockpit :
  1. **Timeline** tokens/jour : 1 ligne = 1 projet, 1 bulle = 1 jour, rayon ∝ tokens du jour ;
  2. **Treemap** par projet : largeur ∝ tokens totaux, pilule coordinateur/sous-agent ;
  3. **Split** coordinateur vs sous-agents delegues + totaux entree/sortie.

**Source (D2)** : *relecture disque* via `iatc-core` (commande `get_history(provider)`) — les JSONL
Claude (`~/.claude/projects`) et rollouts Codex (`~/.codex/sessions`) sont **re-scannes** a chaque
ouverture. **Aucune persistance nouvelle** (pas de SQLite/CouchDB) ; l'historique est **all-time**
(tout le passe present sur le disque des le 1er lancement).

**Portee honnete (D4)** : l'historique est ventile **par projet** et **coord/sub**, a l'echelle du
**provider** — pas filtre par compte : les logs **ne portent pas** l'ID de compte (limitation
`account_ambiguous` du contrat). Un bandeau le rappelle dans la vue. Le **quota en tete** reste, lui,
bien celui du compte.

**Rafraichissement** : au **chargement** + **bouton « Rafraichir »** (re-scan disque). **Aucun
polling** entre deux rafraichissements (l'historique n'a pas besoin d'etre live — le live, c'est le
tray). Empty-state honnete si aucun log du provider (aucune bulle/tuile fantome, sans erreur).

## Configuration (variables d'environnement)

La GUI lit **les memes variables que le daemon** (contrat § 6) pour pointer le meme broker :

| Variable | Defaut | Role |
|---|---|---|
| `IAKATC_MQTT_HOST` | `127.0.0.1` | Hote broker (**iakahub local** par defaut ; surchargeable pour un broker distant) |
| `IAKATC_MQTT_PORT` | `1883` | Port TCP (partage avec iakahub) |
| `IAKATC_MQTT_USER` | — (repli `MOSQUITTO_USER`) | Utilisateur MQTT (optionnel : broker local anonyme) |
| `IAKATC_MQTT_PASSWORD` | — (repli `MOSQUITTO_PASSWORD`) | Mot de passe (**jamais commite** ; optionnel en local) |
| `IAKATC_MQTT_ROOT` | `iakatokencounter` | Racine de topic |
| `IAKATC_MQTT_CLIENT_ID` | `iakatc-tray-<host>` | Identifiant client MQTT |
| `IAKATC_SPAWN_DAEMON` | `true` | Spawner le backbone `iakahub` en sidecar (`false` = subscriber pur) |

## Backbone iakahub en sidecar

Au demarrage, la GUI **spawne `iakahub`** embarque en sidecar (`bundle.externalBin`), sauf si
`IAKATC_SPAWN_DAEMON=false` (cas d'un iakahub deja gere par le systeme / headless). iakahub demarre
le **broker MQTT local** (`127.0.0.1`, anonyme) puis **spawne et supervise `iakatc-daemon`** a cote
de lui (env broker injecte). La GUI et le daemon **ne se parlent que via le broker local**. iakahub
sidecar **s'arrete avec la GUI**, et **termine alors le daemon** (arret en cascade, pas d'orphelin).

Les **deux** binaires doivent exister **avec le suffixe target-triple** attendu par Tauri
(`src-tauri/binaries/iakahub-<triple>` **et** `iakatc-daemon-<triple>` — iakahub localise le daemon
a cote de lui). Les produire avant tout `build`/`tauri dev` :

```bash
bash scripts/prepare-sidecar.sh          # build release iakahub + daemon + copie avec le bon suffixe
```

**Spawn en echec** (binaire absent) : pas de crash — bandeau « backbone indisponible » et la GUI
continue en subscriber pur (utile si un iakahub tourne ailleurs sur le broker). **Port occupe** :
iakahub journalise et sort en code != 0 (fail-fast, pas d'auto-increment ; fixer `IAKATC_MQTT_PORT`).

## Developpement / build

```bash
npm install                  # deps front (une fois)
bash scripts/prepare-sidecar.sh
npm run tauri dev            # app en dev (tray + popover + hot reload front)
npm run tauri build          # bundle applicatif (macOS/Windows/Linux selon l'hote)
bash scripts/quality-report.sh   # gate : typecheck+build front, cargo check/clippy/test workspace
```

> **Multi-OS** : cible macOS + Windows + Linux. Le MVP est developpe/bundle sur **macOS** ; la
> compilation croisee des 3 cibles en CI n'est pas mise en place a ce stade (decision actee).

## Comportement hors-ligne (D5)

- **Broker injoignable** (au demarrage ou perte en cours) : la GUI **reste ouverte**, affiche un
  indicateur « broker deconnecte », garde les dernieres valeurs connues marquees **« perime »**
  (ou « inconnu »/`?` si jamais recues). `rumqttc` retente en tache de fond ; a la reconnexion le
  **retained repeuple** les jauges automatiquement (re-abonnement sur chaque ConnAck).
- **Payload malforme** (hors `{v,t}`) : ignore, sans figer ni planter.

## Rendu de la confiance (D3.1)

| `confidence` | Rendu |
|---|---|
| `official` | jauge pleine, teinte « sur » (vert) |
| `official_stale` | teinte ambre, badge horloge (valeur datee) |
| `local_estimate` | hachure + `~` devant le % (bleu) |
| `none` (ou `remaining_pct` `null`) | grise, `?` a la place du % |

Une fenetre est en plus marquee **« perime »** si sa derniere valeur depasse un seuil de fraicheur
local (defauts 1200 s / 21600 s) ou si la recharge (`resets_at`) est passee.
