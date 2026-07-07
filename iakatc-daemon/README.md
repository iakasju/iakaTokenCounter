# iakatc-daemon

Daemon de mesure **headless** d'iakaTokenCounter : a chaque tick il re-scanne les logs locaux
(Claude Code + Codex), fusionne le quota (statusline + estimation), et **publie l'etat en MQTT
retained code/value** sur le broker Mosquitto d'iakalogs.

> Le **format sur le fil est fige** par [`specs/contrat-mqtt-conso.md`](../specs/contrat-mqtt-conso.md)
> (topics = codes, payload `{"v":<scalaire>,"t":<epoch_s>}`, `retained`, QoS 1). Le daemon publie
> **exactement** selon ce contrat. Cadrage : [`specs/instructions/feature-collecteur-logs.md`](../specs/instructions/feature-collecteur-logs.md).

## Deux modes

```bash
iakatc-daemon                     # boucle de tick (mesure -> fusion -> publication retained)
iakatc-daemon statusline-capture  # capture quota depuis la statusline Claude Code (stdin JSON)
```

## Variables d'environnement

### Broker (contrat § 6)

| Variable | Defaut | Role |
|---|---|---|
| `IAKATC_MQTT_HOST` | `192.168.2.11` | Hote Mosquitto iakabox |
| `IAKATC_MQTT_PORT` | `1883` | Port TCP |
| `IAKATC_MQTT_USER` | — (repli `MOSQUITTO_USER`) | Utilisateur MQTT |
| `IAKATC_MQTT_PASSWORD` | — (repli `MOSQUITTO_PASSWORD`) | Mot de passe MQTT (**jamais commite**) |
| `IAKATC_MQTT_ROOT` | `iakatokencounter` | Racine de topic |
| `IAKATC_MQTT_CLIENT_ID` | `iakatc-daemon-<host>` | Identifiant client MQTT |

### Mesure / stockage

| Variable | Defaut | Role |
|---|---|---|
| `IAKATC_TICK_SECONDS` | `60` | Cadence du tick (re-scan complet) |
| `IAKATC_HOME` | `~/.iakatokencounter` | Racine des fichiers `quota/*.json` et `config.json` |
| `IAKATC_ACCOUNT_LABEL` | `default` | Etiquette de compte (la statusline n'a pas d'ID de compte) |
| `CODEX_HOME` | `~/.codex` | Racine des rollouts Codex |

## Config statusline Claude Code

Brancher la statusline sur la sous-commande de capture. Dans `~/.claude/settings.json` :

```json
{
  "statusLine": {
    "type": "command",
    "command": "IAKATC_ACCOUNT_LABEL=max /chemin/vers/iakatc-daemon statusline-capture"
  }
}
```

La capture lit le JSON statusline sur **stdin**, persiste `IAKATC_HOME/quota/claude.<account>.json`
si `rate_limits` est present, re-emet une ligne minimale sur **stdout** et **ne casse jamais** la
statusline (toujours code 0).

## Fichier de config `IAKATC_HOME/config.json` (optionnel)

```json
{
  "freshness": { "five_hour_seconds": 1200, "seven_day_seconds": 21600 },
  "ceilings": {
    "claude": { "max": { "five_hour_tokens": null, "seven_day_tokens": null } },
    "codex":  { "default": { "five_hour_tokens": null, "seven_day_tokens": null } }
  }
}
```

Plafonds Pro/Max non publies par Anthropic -> `null` par defaut. Tant qu'un plafond est `null`,
l'estimation `used_pct` reste `null` (`confidence:"none"`) mais `used_tokens` est remonte
(choix MVP : ne pas inventer de plafond faux).

## Comportement hors-ligne (standalone)

Broker injoignable : le daemon **continue de mesurer**, journalise, retente (backoff), garde en
memoire le dernier `current` de chaque code, et **republie tout l'etat a la reconnexion** (ConnAck).
Il **ne crashe pas**. Tous les publish sont **retained + QoS 1**.

## Limitations connues (MVP)

- **Quota Codex** best-effort : le plan free expose une fenetre de **30 jours** (`window_minutes:43200`)
  qui ne mappe ni sur 5h ni sur 7d -> non publie tant qu'une fenetre 5h/7d n'apparait pas
  (cf. D3). La conso Codex, elle, est publiee.
- **`.../last`** (periode close) : le MVP publie `current` a chaque tick ; la gestion de `last`
  (cloture de fenetre / jour) est repoussee (cf. compte rendu).
- **`used_tokens` diagnostic du quota** : le JSONL ne porte pas d'`account` -> c'est le total du
  provider (fenetrage 5h/7d non applique au MVP).
