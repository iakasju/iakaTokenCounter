# iakahub — backbone MQTT local du poste (v0)

**iakahub** est le **backbone standalone** d'iakaTokenCounter : un binaire qui **embarque un broker
MQTT local** (`rumqttd`, `127.0.0.1`, anonyme) **et** qui **spawne + supervise** le measure daemon
`iakatc-daemon`. Le poste devient **autonome** : plus aucune dependance a un broker externe
authentifie (le mot de passe du Mosquitto iakabox etant perdu, cf. decision decideur 2026-07-08).

> Cadrage : [`../specs/instructions/feature-iakahub.md`](../specs/instructions/feature-iakahub.md).
> Contrat sur le fil : [`../specs/contrat-mqtt-conso.md`](../specs/contrat-mqtt-conso.md).

## Ce que fait iakahub

```
  iakahub
   ├── broker rumqttd (thread dedie)  ── listener MQTT v4, 127.0.0.1:<port>, anonyme
   └── iatc-daemon (process enfant)   ── env broker injecte, supervise (redemarrage borne)
```

1. **Broker in-process** (`src/broker.rs`) : construit une `rumqttd::Config` depuis un **gabarit
   TOML embarque** (`rumqttd.toml`, `include_str!`) dont le champ `listen` est substitue par
   `127.0.0.1:<port>`. `Broker::start()` est **bloquant** -> lance dans un **thread dedie**. Un seul
   listener **v4**, **aucune auth**. Bind sur la **boucle locale** : jamais expose au reseau.
2. **Orchestration** (`src/supervisor.rs`) : localise `iatc-daemon` **a cote de son propre
   executable** (meme dossier — vrai dans le bundle Tauri comme en dev `target/`), le spawne en lui
   **injectant l'environnement broker**, et le **supervise** (redemarrage borne <= 3, puis abandon
   journalise — le broker reste actif).
3. **Arret propre** (`src/shutdown.rs`) : un handler Ctrl-C / SIGTERM leve un drapeau ; iakahub
   **tue le daemon enfant** avant de sortir -> **aucun orphelin** (arret en cascade).

### Environnement injecte au daemon

Le broker local etant anonyme, les creds sont **factices** (ignores) — cela satisfait l'exigence
« creds obligatoires » du daemon **sans modifier son code** :

| Variable | Valeur |
|---|---|
| `IAKATC_MQTT_HOST` | `127.0.0.1` |
| `IAKATC_MQTT_PORT` | `<port iakahub>` |
| `IAKATC_MQTT_USER` | `iakahub` (factice) |
| `IAKATC_MQTT_PASSWORD` | `local` (factice) |

## Configuration

| Variable | Defaut | Role |
|---|---|---|
| `IAKATC_MQTT_PORT` | `1883` | Port TCP du broker local (bind fixe `127.0.0.1`) |
| `RUST_LOG` | `info` | Niveau de log (`tracing`) |

**Port deja occupe** : iakahub **journalise une erreur explicite** et **sort en code != 0**
(fail-fast deterministe, **pas d'auto-increment**). Fixer un autre port via `IAKATC_MQTT_PORT`.

## Codes de sortie

| Code | Cas |
|---|---|
| `0` | Arret propre sur signal (daemon termine, aucun orphelin) |
| `2` | Broker non demarre (port occupe, ou pas a l'ecoute apres le delai) |
| `3` | Handler d'arret non installable |

## Lancer / tester

```bash
cargo run -p iakahub                 # broker local + supervision du daemon voisin
IAKATC_MQTT_PORT=21883 cargo run -p iakahub   # sur un autre port
cargo test -p iakahub                # round-trip retained in-process + supervision + resolveur
```

Verifier a la main que le broker ecoute (avec le daemon a cote, les jauges du tray se remplissent
en local) :

```bash
# round-trip retained sans broker externe, en local
mosquitto_sub -h 127.0.0.1 -p 1883 -t 'iakatokencounter/#' -v
```

## Hors scope (v0)

Bridge vers le Mosquitto iakabox (`192.168.2.11`), auth/TLS, listener WebSocket, listener MQTT v5,
absorption des logs, routage de conversations, persistance, auto-increment de port. Voir la section
« Hors scope » de l'instruction.
