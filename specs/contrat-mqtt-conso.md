# Contrat MQTT — conso / limits / quota (iakaTokenCounter → iakalogs)

> Rédigé par Gandalf (P1 — cadrage). **Interface partagée** entre le **producteur** (le daemon
> de mesure iakaTokenCounter), le **broker** (Mosquitto d'iakalogs/iakaboxlogs, non modifié) et
> les **subscribers** (IakaCockpit widgets `economy`/`log`, future GUI tray).
>
> Ce document **fige le format sur le fil**. Tant qu'il n'évolue pas (version du contrat),
> producteur et consommateurs peuvent être développés et déployés indépendamment.

---

## 0. Principes verrouillés (décideur, cf. `PROJET.md`)

- **Modèle code/value** : **le topic est le code** (l'adresse pleinement qualifiée d'**une**
  grandeur scalaire) ; **le payload est la valeur** (`{"v":<scalaire>,"t":<epoch_s>}`). **Aucun
  gros objet JSON** par message. Objectif : ingestion/stockage rapides + retained → un client lit
  la valeur d'un code **sans parser de structure**.
- **On ne touche pas au dépôt iakaboxlogs.** Un broker MQTT accepte n'importe quel topic sans
  configuration : le daemon **publie**, c'est tout. Aucune modif broker requise.
- **Deux axes d'agrégation** : `.../all/projets/agents/...` (projet × agent) ET
  `.../all/ia/agents/...` (fournisseur IA × agent).
- **Messages `retained`** pour l'état `current`/`last` : tout subscriber lit l'état sans rejouer
  les logs bruts ni recalculer.
- **Persistance CouchDB des métriques = hors scope** (elle toucherait le pont iakaboxlogs). Le MVP
  se limite aux messages MQTT retained (voir § 6 : racine distincte → le pont ne les aspire pas).

---

## 1. Racine de topic — choix et justification

**Racine retenue : `iakatokencounter/`** (segment de tête distinct). **Validée décideur.**

**Pourquoi une racine dédiée plutôt que de se greffer sous `iakaboxlogs/`** :
1. **Non-collision de namespace** : le pont CouchDB d'iakalogs est abonné à `iakaboxlogs/#`
   (README iakaboxlogs). Publier sous `iakatokencounter/` garantit que nos métriques **ne sont
   pas** aspirées par le pont → cohérent avec « persistance hors scope » (§ 0). On réutilise le
   **broker**, pas le **pipeline de persistance**.
2. **Lisibilité** : `iakaboxlogs/<royaume>/<agent>/<conv_id>` = *conversations* ;
   `iakatokencounter/...` = *codes de conso/quota*. Deux domaines, deux racines.
3. **Réversibilité** : le jour où le décideur voudra persister les métriques, il suffira de
   pointer un consumer sur `iakatokencounter/#` — sans avoir pollué `iakaboxlogs/#` entre-temps.

> Le segment `all/` (imposé par les deux axes du décideur) désigne l'agrégat « tout le poste » ;
> il laisse la place à un futur segment par machine/hôte (`iakatokencounter/<host>/...`) sans casser
> le contrat — hors scope MVP.

---

## 2. Arbre de topics — chaque feuille est UN code scalaire

**Principe** : le **code** est le chemin qui identifie une grandeur ; le suffixe terminal
`current`/`last` sélectionne l'**état** (§ 4). L'exemple raccourci du décideur
`.../quota/5h/remaining_pct` désigne implicitement `.../quota/5h/remaining_pct/current`.

```
iakatokencounter/
├── all/
│   ├── projets/agents/{project}/{agent}/conso/     ← AXE 1 : projet × agent
│   │   ├── input_tokens/{current|last}              (number)
│   │   ├── output_tokens/{current|last}             (number)
│   │   ├── cache_tokens/{current|last}              (number)
│   │   └── used_tokens/{current|last}               (number, total economy.rs)
│   │
│   └── ia/
│       ├── agents/{provider}/{agent}/conso/         ← AXE 2 : IA × agent
│       │   ├── input_tokens/{current|last}          (number)
│       │   ├── output_tokens/{current|last}         (number)
│       │   ├── cache_tokens/{current|last}          (number)
│       │   └── used_tokens/{current|last}           (number)
│       │
│       └── {provider}/{account}/
│           ├── quota/5h/
│           │   ├── used_pct/{current|last}          (number 0..100 | null)
│           │   ├── remaining_pct/{current|last}     (number 0..100 | null)
│           │   ├── used_tokens/{current|last}       (number | null, diagnostic)
│           │   ├── resets_at/current                (number, epoch s)
│           │   ├── captured_at/current              (number, epoch s | null)
│           │   ├── confidence/current               (string, voir § 3.3)
│           │   └── source/current                   (string, voir § 3.3)
│           ├── quota/7d/  … (MÊMES codes que 5h)
│           └── limits/
│               ├── ceiling_5h_tokens/current        (number | null)
│               └── ceiling_7d_tokens/current        (number | null)
│
└── meta/daemon/
    ├── state/current                                (string: "up")
    ├── last_tick_at/current                         (number, epoch s)
    ├── broker_connected/current                     (boolean)
    └── version/current                              (string)
```

### Valeurs des variables de topic

| Variable | Valeurs MVP | Source |
|---|---|---|
| `{provider}` | `claude`, `codex` | fixe (MVP = ces 2 sources ; `claude` = Claude Code) |
| `{project}` | dernier segment du `cwd` (ex. `iakaTokenCounter`) | JSONL Claude / rollout Codex |
| `{agent}` | `coordinator`, `subagent` | `isSidechain` des JSONL Claude ; Codex → `coordinator` seul |
| `{account}` | étiquette manuelle (`default`, `max`, `pro`…) | env `IAKATC_ACCOUNT_LABEL` (config statusline) |

> **`{agent}` (validé décideur — personas repoussés)** : les JSONL ne portent pas l'identité de
> persona. Seule distinction mesurable = **coordinateur vs sous-agent délégué** (`isSidechain`),
> déjà calculée par `economy.rs`. Codex n'a pas de sidechain → `{agent} = coordinator`.

### Exemples de topics feuilles pleins (concrets)

| Besoin | Topic (code) | Payload |
|---|---|---|
| Quota 5 h **restant** du compte Claude Max | `iakatokencounter/all/ia/claude/max/quota/5h/remaining_pct/current` | `{"v":87.5,"t":1751894400}` |
| Quota 5 h **utilisé** (compte Claude Max) | `iakatokencounter/all/ia/claude/max/quota/5h/used_pct/current` | `{"v":12.5,"t":1751894400}` |
| Recharge de la fenêtre 5 h | `iakatokencounter/all/ia/claude/max/quota/5h/resets_at/current` | `{"v":1751864400,"t":1751894400}` |
| Confiance de la mesure 5 h | `iakatokencounter/all/ia/claude/max/quota/5h/confidence/current` | `{"v":"official","t":1751894400}` |
| **input** tokens du projet `iakaTokenCounter` par le coordinateur | `iakatokencounter/all/projets/agents/iakaTokenCounter/coordinator/conso/input_tokens/current` | `{"v":90000,"t":1751894400}` |
| **used** tokens des sous-agents sur `IakaCockpit` | `iakatokencounter/all/projets/agents/IakaCockpit/subagent/conso/used_tokens/current` | `{"v":123456,"t":1751894400}` |
| **used** tokens totaux attribués à Codex | `iakatokencounter/all/ia/agents/codex/coordinator/conso/used_tokens/current` | `{"v":45000,"t":1751894400}` |
| Plafond 7 j configuré (Claude Max) | `iakatokencounter/all/ia/claude/max/limits/ceiling_7d_tokens/current` | `{"v":null,"t":1751894400}` |
| Santé du daemon | `iakatokencounter/meta/daemon/state/current` | `{"v":"up","t":1751894400}` |

> **Abonnements typiques** :
> - GUI tray (jauges par compte) : `iakatokencounter/all/ia/+/+/quota/#`
> - Cockpit widget `economy` (par projet) : `iakatokencounter/all/projets/agents/#`
> - Un code précis : `iakatokencounter/all/ia/claude/max/quota/5h/remaining_pct/current`

---

## 3. Payload — valeur scalaire `{v, t}`

**Format unique de tout message** :
```json
{ "v": <scalaire>, "t": 1751894400 }
```
- `v` : la valeur du code — `number` | `string` | `boolean` | `null`. **Jamais** un objet ni un
  tableau (si on a besoin d'une structure, c'est qu'il manque un code : on décompose).
- `t` : **epoch secondes** (UTC) de la **lecture** qui a produit cette valeur.

**Micro-choix tranché : `{v,t}` plutôt que valeur nue.** Une valeur retained nue (`87.5`) ne dit pas
**quand** elle a été lue → un client ne peut pas juger sa fraîcheur (2 s ou 2 jours ?). Le `t`
embarqué reste minimal (2 champs, zéro imbrication) tout en rendant la péremption calculable côté
client. `schema_version` n'est **pas** répété dans chaque message (surcoût sur un modèle scalaire) :
il est **porté par la version du contrat** ; un changement de forme = nouvelle version documentée ici
(contrat **v1**).

**Valeur `null`** : un code mesuré mais **inconnu** (ex. `used_pct` sans plafond configuré) est publié
`{"v":null,"t":…}` — un « inconnu **daté** », distinct de l'**absence** de topic (jamais mesuré). Pour
**effacer** un code (projet/agent disparu), publier un **payload vide 0 octet en retained** (convention
MQTT).

### 3.1 Codes de conso (numériques, sur les DEUX axes)

| Code | Type | Sens |
|---|---|---|
| `input_tokens` | number | `input + cache_creation + cache_read` (règle economy.rs) |
| `output_tokens` | number | tokens de sortie |
| `cache_tokens` | number | `cache_creation + cache_read` (diagnostic) |
| `used_tokens` | number | total retenu affichage = `input_tokens + output_tokens` |

Sur l'**axe ia** (`ia/agents/{provider}/{agent}/…`), ce sont les mêmes tokens **re-sommés par
`(provider, agent)`** (tous projets). Sur l'**axe projet**, ils sont ventilés par `{project}`.

### 3.2 Codes de quota (par `{provider}/{account}/quota/{5h|7d}/…`)

| Code | Type | Sens |
|---|---|---|
| `used_pct` | number\|null | 0..100, `null` si inconnu |
| `remaining_pct` | number\|null | `100 - used_pct`, `null` si `used_pct` null |
| `used_tokens` | number\|null | comptage JSONL de la fenêtre (diagnostic) |
| `resets_at` | number | epoch **s** de la prochaine recharge |
| `captured_at` | number\|null | epoch s de la capture statusline (fraîcheur) |
| `confidence` | string | `official` \| `official_stale` \| `local_estimate` \| `none` |
| `source` | string | `statusline` \| `jsonl_estimate` \| `codex_rollout` \| `config` |

### 3.3 `confidence` / `source` — publiés comme **codes propres, à côté des valeurs**

> **Note (exigée décideur)** : `confidence` et `source` ne sont **pas** noyés dans un objet Reservoir.
> Ce sont des **codes scalaires à part entière**, publiés **en parallèle** des codes de valeur, sur le
> même préfixe de fenêtre. Un subscriber qui affiche `remaining_pct/current` lit **en plus**
> `confidence/current` (même chemin, code voisin) pour teinter sa jauge (officiel / estimé / périmé).
> Idem `source` pour tracer la provenance. Ils suivent le même format `{v,t}` (ici `v` est une chaîne).

### 3.4 Codes de limits & meta

| Code | Type | Emplacement |
|---|---|---|
| `ceiling_5h_tokens` / `ceiling_7d_tokens` | number\|null | `.../{provider}/{account}/limits/…/current` |
| `state` | string (`"up"`) | `meta/daemon/state/current` |
| `last_tick_at` | number (epoch s) | `meta/daemon/last_tick_at/current` |
| `broker_connected` | boolean | `meta/daemon/broker_connected/current` |
| `version` | string | `meta/daemon/version/current` |

> Plafonds Pro/Max non publiés par Anthropic → `null` par défaut (choix MVP assumé : ne pas inventer
> de plafond faux). Tant qu'ils sont `null`, l'estimation `used_pct` reste `null`.

---

## 4. Politique retained : `current` vs `last`

**Sémantique retained (vérifiée)** : le broker conserve **un** message par topic, l'**écrase** à
chaque nouveau publish, et le sert **immédiatement** à tout nouvel abonné. Modèle « dernière valeur
connue » — exactement ce que veut un code scalaire retained.

| Suffixe | Sémantique | Écriture | Retained |
|---|---|---|---|
| `current` | **valeur vivante** du code au tick courant | **écrasée à chaque tick** | oui |
| `last` | **dernière valeur close** du code avant réinitialisation de sa période | écrite **une fois par clôture** | oui |

- **quota `…/{code}/last`** : à la recharge d'une fenêtre (`now > resets_at`), le daemon copie le
  **dernier `current` connu** des codes quantitatifs (`used_pct`, `remaining_pct`, `used_tokens`) vers
  leur `…/last`, puis repart à zéro sur `current`. Les codes `confidence`/`source`/`resets_at`/
  `captured_at` restent **`current` uniquement**.
- **conso `…/{code}/last`** : valeur de la **période close** (jour précédent) — la ventilation par
  jour existe déjà dans `economy.rs`. `current` = cumul du jour courant.

> **Micro-choix tranché** : `last` = « période précédente close » (pas « avant-dernier tick »). C'est
> la lecture utile pour un humain (« mes tokens d'hier », « mon quota avant recharge »).

**QoS = 1 (AtLeastOnce)** pour **tous** les topics. QoS 0 ne garantit pas la livraison (un tick perdu
= code figé) ; QoS 2 ajoute un aller-retour inutile pour de la télémétrie idempotente (chaque
`current` écrase le précédent). QoS 1 est le compromis retenu.

**Rétention / expiration logique** : MQTT ne périme pas un retained tout seul. La péremption est
portée par le **`t`** du payload — un subscriber considère un `current` comme **périmé** si `t`
dépasse son seuil de fraîcheur local, ou (pour le quota) si `now > resets_at/current`. Purge d'un code
= **payload vide 0 octet en retained**.

---

## 5. Timestamps & format

- **Tous les horodatages sont en epoch secondes (entier UTC)** : le `t` de chaque payload, ainsi que
  les codes-valeurs temporels (`resets_at`, `captured_at`, `last_tick_at`). Cohérent avec le canal
  statusline de Claude Code (`resets_at` déjà en epoch s).
- `t` = instant de lecture/production par le daemon ; il **date la valeur**, même quand la valeur
  elle-même est un epoch (ex. `resets_at`).
- Pas d'ISO 8601 sur le fil (le JSONL Claude l'utilise en interne ; le daemon convertit en epoch s).

---

## 6. Coordonnées broker, hors-ligne, configurabilité

**Broker (Mosquitto iakabox, existant)** :

| Paramètre | Valeur défaut | Variable d'env |
|---|---|---|
| Hôte | `192.168.2.11` | `IAKATC_MQTT_HOST` |
| Port TCP | `1883` | `IAKATC_MQTT_PORT` |
| Utilisateur | (obligatoire) | `IAKATC_MQTT_USER` (repli `MOSQUITTO_USER`) |
| Mot de passe | (obligatoire) | `IAKATC_MQTT_PASSWORD` (repli `MOSQUITTO_PASSWORD`) |
| Racine de topic | `iakatokencounter` | `IAKATC_MQTT_ROOT` |
| Client id | `iakatc-daemon-<host>` | `IAKATC_MQTT_CLIENT_ID` |

- **Secrets jamais commités** : identifiants via env uniquement (convention iakaframe).
- **WS `9883`** disponible côté broker (README iakaboxlogs) pour un futur subscriber navigateur ; le
  daemon publie en **TCP 1883**.

**Comportement hors-ligne (standalone, ne crashe pas)** :
- Broker injoignable : le daemon **continue de mesurer**, garde en mémoire le dernier `current` de
  chaque code, **journalise**, **retente** (backoff borné). À la reconnexion, il **republie** l'état
  courant (retained) pour resynchroniser les subscribers.
- Un tick manqué est rattrapé au tick suivant (les codes sont **recalculés depuis les logs**, pas
  incrémentés en mémoire volatile).

---

## 7. Cohabitation avec iakaboxlogs

| Aspect | iakaboxlogs (conversations) | iakaTokenCounter (codes de mesure) |
|---|---|---|
| Racine topic | `iakaboxlogs/<royaume>/<agent>/<conv_id>` | `iakatokencounter/all/...` |
| Payload | objet `{role,content,…}` | scalaire `{v,t}` |
| Persistance | pont n8n → CouchDB (abonné `iakaboxlogs/#`) | **aucune** (hors scope MVP) |
| Retained | non (flux d'événements) | **oui** (dernière valeur par code) |
| Producteur | agents (skill `log-conversation`) | daemon iakaTokenCounter |

**Rapport** : même **broker physique**, **arbres disjoints**. Le pont CouchDB (`iakaboxlogs/#`)
**n'intercepte pas** `iakatokencounter/#` → pas de persistance accidentelle, pas de collision, aucune
modif du dépôt iakaboxlogs. Le vocabulaire d'`agent` est partagé (coordinator/subagent, et à terme les
personas) : une future corrélation conversations ↔ conso reste possible sans changer ce contrat.

---

## Sources (veille)

- rumqttc (client MQTT Rust, retained + QoS, maj nov. 2025) : https://crates.io/crates/rumqttc
- MQTT retained — sémantique « dernière valeur connue » (HiveMQ Essentials Part 8) :
  https://www.hivemq.com/blog/mqtt-essentials-part-8-retained-messages/
- Claude Code statusline `rate_limits` (`used_percentage`, `resets_at` epoch s, pas d'ID de compte) :
  https://code.claude.com/docs/en/statusline
- Conventions topics/payload iakaboxlogs : `iakaboxlogs/README.md`
