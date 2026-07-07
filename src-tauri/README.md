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
  daemon iakatc-daemon (sidecar) ──publish retained──►  broker Mosquitto (iakalogs)
                                                              │  subscribe (retained, QoS 1)
                                                              ▼
   backend Rust (rumqttc, thread)  ──etat en memoire──►  ReservoirCard[]
        │  evenement tray://state  +  commande get_reservoirs
        ▼
   webview (popover TS)  ──rend les cartes / jauges / badges de confiance
```

- **Backend Rust** (`src/mqtt_sub.rs`, `src/state.rs`) : seul a parler MQTT. Abonnements
  `…/all/ia/+/+/quota/#` (jauges, decouverte dynamique des comptes) et `…/meta/daemon/#`.
- **Tray** (`src/tray.rs`) : icone simple + tooltip du **pire reservoir**, clic gauche =
  popover, menu droit = Ouvrir / Quitter.
- **Sidecar** (`src/lib.rs`) : le daemon est **spawne** au demarrage (voir plus bas).
- **Hook analytics** (`src/analytics.rs`) : double-clic sur une carte -> fenetre stub « A venir »
  (le vrai analytics est une instruction ulterieure).

## Configuration (variables d'environnement)

La GUI lit **les memes variables que le daemon** (contrat § 6) pour pointer le meme broker :

| Variable | Defaut | Role |
|---|---|---|
| `IAKATC_MQTT_HOST` | `192.168.2.11` | Hote Mosquitto |
| `IAKATC_MQTT_PORT` | `1883` | Port TCP |
| `IAKATC_MQTT_USER` | — (repli `MOSQUITTO_USER`) | Utilisateur MQTT |
| `IAKATC_MQTT_PASSWORD` | — (repli `MOSQUITTO_PASSWORD`) | Mot de passe (**jamais commite**) |
| `IAKATC_MQTT_ROOT` | `iakatokencounter` | Racine de topic |
| `IAKATC_MQTT_CLIENT_ID` | `iakatc-tray-<host>` | Identifiant client MQTT |
| `IAKATC_SPAWN_DAEMON` | `true` | Spawner le daemon en sidecar (`false` = subscriber pur) |

## Daemon en sidecar (D1)

Au demarrage, la GUI **spawne `iakatc-daemon`** embarque en sidecar (`bundle.externalBin`), sauf
si `IAKATC_SPAWN_DAEMON=false` (cas d'un daemon deja gere par le systeme / headless). La GUI et le
daemon **ne se parlent que via le broker**. Le daemon sidecar **s'arrete avec la GUI** (limite
assumee au MVP).

Le binaire doit exister **avec le suffixe target-triple** attendu par Tauri
(`src-tauri/binaries/iakatc-daemon-<triple>`). Le produire avant tout `build`/`tauri dev` :

```bash
bash scripts/prepare-sidecar.sh          # build release du daemon + copie avec le bon suffixe
```

**Spawn en echec** (binaire absent) : pas de crash — bandeau « daemon indisponible » et la GUI
continue en subscriber pur (utile si un daemon tourne ailleurs sur le broker).

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
