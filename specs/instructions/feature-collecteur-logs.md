# Instruction : Daemon de mesure v0 (conso/quota → MQTT retained)

> Rédigé par Gandalf (P1 — cadrage). Consommé par Gimli comme instruction de travail.
> Première brique livrable du cap « iakalogs central de coms, iakaTokenCounter daemon de mesure ».
> **Interface de sortie figée** par `specs/contrat-mqtt-conso.md` (Livrable frère) — à lire AVANT.

---

## Contexte

iakaTokenCounter devient un **daemon de mesure headless** (cf. `PROJET.md` § « Cap (nord) »).
Il **mesure** la consommation / le quota de tokens sur le poste et **publie** l'état dans le
broker Mosquitto **existant** d'iakalogs (iakaboxlogs), en messages **retained**, selon deux axes
(projet × agent, IA × agent). IakaCockpit et la future GUI tray s'y **abonnent** ; ils ne
recalculent rien.

Cette instruction ferme le périmètre du **daemon v0** : mesurer + agréger + publier. Elle
**remplace** l'ancienne approche « collecteur TS/ccusage » : le cœur de mesure existe déjà en
**Rust testé** (`IakaCockpit/src-tauri/src/economy.rs`) → **on le réutilise, ccusage est
abandonné** (décision journal `PROJET.md`, 2026-07-07).

### Faits vérifiés (veille Gandalf, sources en bas)

- **`economy.rs` (Rust, testé)** parse les JSONL de session Claude Code
  (`~/.claude/projects/<escaped>/<sid>.jsonl`), somme `message.usage`
  (`input + cache_creation + cache_read + output`) **par projet** (dernier segment du `cwd`) et
  **sépare coordinateur vs sous-agent** via `isSidechain`. Fonctions **pures et testables** :
  `project_of`, `fold_line`/`Acc`, `finalize`, `scan_projects_dir`, plus la ventilation **par
  jour** (`fold_activity_line`, `scan_projects_activity`). Lecture seule, défensif (ligne invalide
  ignorée, jamais de panique).
- **`codex.rs` (Rust)** taile le rollout Codex (`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`)
  mais **ignore explicitement** l'événement porteur d'usage :
  `{"type":"event_msg","payload":{"type":"token_count", …}}` (commentaire « `token_count` … →
  ignoré »). **La conso Codex n'est donc captée par personne** aujourd'hui.
- **Statusline Claude Code** : seul canal du **quota exact** (`rate_limits.five_hour` /
  `.seven_day`, `used_percentage` 0–100, `resets_at` epoch **secondes**). Présent **seulement**
  pour Pro/Max, **après la 1ʳᵉ réponse API**, chaque fenêtre pouvant manquer. **Aucun identifiant
  de compte ni projet** dans ce JSON → étiquetage manuel du compte requis. C'est un **script
  shell** que Claude Code appelle avec le JSON sur **stdin**.
- **rumqttc** (client MQTT Rust, maj nov. 2025, maintenu) supporte **publish retained + QoS 1**,
  event-loop tokio. C'est le client retenu pour publier.
- **MQTT retained** = le broker garde **une** valeur par topic, l'écrase à chaque publish, la sert
  aux nouveaux abonnés → modèle « dernière valeur connue » exactement voulu pour `current`/`last`.

## Ce qui existe

| Élément | Où | État |
|---|---|---|
| Vision + cap + décisions | `specs/PROJET.md` | à jour |
| **Contrat MQTT** (topics, payloads, retained) | `specs/contrat-mqtt-conso.md` | figé (Livrable frère) |
| Cœur de mesure Claude Code | `IakaCockpit/src-tauri/src/economy.rs` | **existe, testé** — à **porter (copie Rust→Rust)** |
| Tailer Codex (paroles/gestes) | `IakaCockpit/src-tauri/src/codex.rs` | existe ; **ignore `token_count`** → à compléter |
| Broker Mosquitto | iakabox `192.168.2.11:1883` | déployé, **non modifiable** |
| Dossier code local | `src/` | vide (`.gitkeep`) |
| Dossier mocks | `specs/mock/` | vide (`.gitkeep`) |
| Daemon / binaire | — | **absent (objet de cette instruction)** |

## Décision

### D1 — Le daemon est un **binaire Rust** ; le cœur de mesure est une **crate lib** réutilisable

**Retenu** : deux crates dans un workspace Cargo à la racine du dépôt.
- **`iakatc-core`** (lib) : logique **pure et testable** — parsing JSONL (porté d'`economy.rs`),
  comptage Codex, capture/fusion quota, agrégation par axe, **construction des topics-codes et
  sérialisation des payloads scalaires `{v,t}`** du contrat. **Aucune dépendance Tauri.**
- **`iakatc-daemon`** (bin) : boucle de vie (tick, connexion MQTT via rumqttc, hors-ligne/backoff)
  + sous-commande de capture statusline. Fin, orchestre `iakatc-core`.

**Pourquoi** : le cap prévoit un daemon **réutilisable par la GUI Tauri d'iakaTokenCounter** et
**vendorable dans IakaCockpit**. Un cœur lib séparé du binaire rend les deux possibles sans
réécriture. Rust car `economy.rs` est en Rust (copie Rust→Rust, zéro friction).

**Écarté** :
- *Collecteur TS/Node + ccusage* (ancienne instruction) : abandonné par décision décideur (le
  cœur Rust existe et est testé ; pas de runtime Node à embarquer).
- *Tout mettre dans le binaire* : empêcherait le vendoring lib dans Cockpit / la GUI.

### D2 — `economy.rs` est **copié** (vendored Rust→Rust), pas référencé en submodule

**Retenu** : **copier** les fonctions pures d'`economy.rs` dans `iakatc-core` (module
`measure/claude.rs`), en **retirant les wrappers `#[tauri::command]`** (`portfolio_economy`,
`portfolio_activity`) inutiles ici. On garde : `project_of`, `fold_line`/`Acc`, `finalize`,
`scan_projects_dir`, `fold_activity_line`/`ActAcc`, `finalize_activity`, `scan_projects_activity`,
`claude_projects_dir` **et les tests unitaires associés** (ils valident le portage).

**Pourquoi une copie assumée et non un crate partagé maintenant** : `economy.rs` vit aujourd'hui
**dans** `IakaCockpit/src-tauri` (couplé au binaire Tauri de Cockpit). En extraire un crate publié
partagé est un chantier transverse (touche Cockpit) **hors scope v0**. La direction du cap est
l'inverse (vendorer *notre* daemon **dans** Cockpit plus tard). Donc : copie ici, **source de
vérité documentée** en tête de fichier (`// porté de IakaCockpit/src-tauri/src/economy.rs @ <sha>`).

**Micro-choix tranché** : copie (avec en-tête de provenance + tests portés), pas submodule.

### D3 — Comptage **Codex** : compléter ce que `codex.rs` ignore

**Retenu** : nouveau module `iakatc-core/measure/codex.rs` qui **scanne les rollouts**
(`~/.codex/sessions/**/*.jsonl`, `CODEX_HOME` respecté, défaut `~/.codex`) et **agrège les
tokens** depuis les événements **`token_count`** que `codex.rs` laisse tomber. Le `cwd` du projet
est lu dans le `session_meta` en tête de rollout (déjà fait par `rollout_cwd` dans `codex.rs` — on
en calque l'esprit).

**Parse défensif obligatoire (shape à confirmer en recette)** : le contenu de
`event_msg.payload.token_count.info` (champs de tokens, éventuel `rate_limits`) n'a **pas** été
figé au spike Codex (cf. les avertissements « shapes à confirmer » de `codex.rs`). Gimli **parse
défensivement** (champ absent/inconnu → ignoré, jamais de panique) et **confirme le schéma réel sur
un vrai rollout** avant de clore. Fixture réelle attendue dans `specs/mock/`.

**Micro-choix tranché** : Codex n'a **pas** de notion `isSidechain` → l'agent Codex est
**`coordinator`** (un seul bucket). Le quota Codex est **best-effort** : si `token_count` porte un
`rate_limits`, on le remonte (source `codex_rollout`) ; sinon quota Codex = estimation/`none`.

### D4 — Capture **quota statusline** : sous-commande du **binaire Rust** (plus de Node)

**Retenu** : la statusline est branchée sur `iakatc-daemon statusline-capture`. Ce mode :
1. lit le **JSON statusline sur stdin**,
2. si `rate_limits` présent, **persiste** un fichier `quota/<provider>.<account>.json` sous
   `IAKATC_HOME` (défaut `~/.iakatokencounter/`), **un fichier par compte** (évite les races),
3. **ré-émet une ligne d'affichage minimale sur stdout** (pass-through, ne casse pas la statusline),
4. **ne plante jamais** (échec silencieux, code 0) — la statusline reste fonctionnelle.

**Pourquoi Rust et pas le script Node de l'ancienne instruction** : ccusage/Node abandonnés ; un
seul binaire couvre daemon **et** capture → zéro dépendance de runtime, déploiement trivial.

**Schéma du fichier quota** (inchangé vs ancienne instruction, contrat de fichier stable) :
```json
{
  "account": "max",
  "provider": "claude",
  "captured_at": 1751846100,
  "source_version": "2.1.90",
  "rate_limits": {
    "five_hour": { "used_percentage": 23.5, "resets_at": 1751864400 },
    "seven_day": { "used_percentage": 41.2, "resets_at": 1752451200 }
  }
}
```
- `<account>` vient de `IAKATC_ACCOUNT_LABEL` (défaut `default`), posé par l'utilisateur dans sa
  config statusline (étiquetage manuel multi-comptes — la statusline n'a pas d'ID de compte).
- `captured_at` = epoch **secondes**. Fenêtre absente → **omise** (pas écrite à `null`).

### D5 — Fusion **hybride** du quota (par compte × fenêtre) → le Réservoir

Pour chaque `(account, provider, window ∈ {5h,7d})`, choisir **une** valeur + **étiqueter** la
confiance (logique portée de l'ancienne instruction, désormais en Rust) :

1. **Exact frais** → `confidence:"official"`, `source:"statusline"` : fichier quota présent, fenêtre
   présente, `resets_at > now`, `captured_at` plus récent que le seuil de fraîcheur
   (`FRESH_MAX_AGE_5H` défaut **20 min**, `FRESH_MAX_AGE_7D` défaut **6 h**).
2. **Exact périmé** → `confidence:"official_stale"` : idem mais `captured_at` au-delà du seuil.
3. **Estimation** → `confidence:"local_estimate"`, `source:"jsonl_estimate"` : sinon, tokens cumulés
   sur la fenêtre (mesure JSONL) ÷ **plafond configuré**, `used_percentage = min(100, tokens/ceiling*100)`.
4. **Aucune** → `confidence:"none"`, `used_percentage:null` : ni exact utilisable, ni plafond. On
   remonte quand même `used_tokens` en diagnostic.

Seuils et plafonds **configurables** via `IAKATC_HOME/config.json` (tout optionnel) :
```json
{
  "freshness": { "five_hour_seconds": 1200, "seven_day_seconds": 21600 },
  "ceilings": {
    "claude": { "default": { "five_hour_tokens": null, "seven_day_tokens": null } },
    "codex":  { "default": { "five_hour_tokens": null, "seven_day_tokens": null } }
  }
}
```
> Plafonds Pro/Max non publiés par Anthropic → `null` par défaut ; tant qu'ils sont `null`,
> `used_percentage` estimé reste `null` (`confidence:"none"`) mais `used_tokens` est remonté.
> **Choix MVP assumé** : ne pas inventer de plafond faux.

**Pièges connus** (drapeaux `notes[]` du payload quota, non résolus au MVP — juste documentés) :
`placeholder_input_tokens`, `account_ambiguous`, `reset_anchor_first_prompt`, `tokenizer_shift`.

### D6 — Agrégation selon les **deux axes**, publiée en **code/value scalaire**

À chaque tick, `iakatc-core` produit, **par grandeur**, **un couple (topic-code, valeur scalaire)**
selon le contrat (topic = code pleinement qualifié ; payload = `{"v":<scalaire>,"t":<epoch_s>}`) :
- **Axe projet × agent** : pour chaque `(project, agent∈{coordinator,subagent})` mesuré, **un code
  par grandeur** → `all/projets/agents/{project}/{agent}/conso/{input_tokens|output_tokens|cache_tokens|used_tokens}/current`.
- **Axe IA × agent** : mêmes tokens **re-sommés par `(provider, agent)`** (tous projets) →
  `all/ia/agents/{provider}/{agent}/conso/{code}/current`. `provider` = `claude` (Claude Code) ou `codex`.
- **Quota** : le `Reservoir` de D5 est **décomposé en codes scalaires atomiques** (pas d'objet sur le
  fil) → `all/ia/{provider}/{account}/quota/{5h|7d}/{used_pct|remaining_pct|used_tokens|resets_at|`
  `captured_at|confidence|source}/current`. `confidence`/`source` sont des **codes voisins** publiés
  **à côté** des valeurs (contrat § 3.3), pas noyés dans un objet.
- **Limits** : `all/ia/{provider}/{account}/limits/{ceiling_5h_tokens|ceiling_7d_tokens}/current`.
- **Santé** : `meta/daemon/{state|last_tick_at|broker_connected|version}/current`.

La **construction des topics-codes et des payloads `{v,t}` suit strictement `contrat-mqtt-conso.md`**
(§ 2 et § 3). Le mapping (structures Rust → `(topic, {v,t})`) vit dans
`iakatc-core/publish/contract.rs`, **testé** contre les exemples du contrat. Une valeur inconnue est
publiée `{"v":null,"t":…}` (contrat § 3) ; jamais d'objet composite.

### D7 — Boucle de vie du daemon : tick + retained + hors-ligne

- **Tick périodique** (défaut **60 s**, `IAKATC_TICK_SECONDS`) : re-scan complet des logs (les
  compteurs sont **recalculés depuis le disque**, pas incrémentés en mémoire → un tick manqué se
  rattrape tout seul), fusion quota, publication retained `current`.
- **`current` vs `last`** (contrat § 4) : `current` écrasé chaque tick ; `last` écrit **à la
  clôture** d'une période (fenêtre quota rechargée `now>resets_at` → copie du dernier `current` des
  codes quantitatifs vers leur `.../last` ; jour calendaire changé → conso de la veille vers
  `.../{code}/last`).
- **Hors-ligne** (exigence standalone) : broker injoignable → le daemon **continue de mesurer**,
  **journalise**, **retente** (backoff borné), et **republie l'état courant à la reconnexion**. Il
  **ne crashe pas**. QoS 1 pour tous les publish.

## Étapes d'implémentation

1. **Workspace Cargo** à la racine : crates `iakatc-core` (lib) et `iakatc-daemon` (bin). Runner de
   test = `cargo test`. Dépendances : `serde`/`serde_json`, `rumqttc`, `dirs`, `tokio` (event-loop
   rumqttc), un env-reader léger.
2. **Porter `economy.rs`** → `iakatc-core/measure/claude.rs` (D2) : copier fonctions pures + tests,
   retirer les `#[tauri::command]`, ajouter l'en-tête de provenance (`// porté de … @ <sha>`).
3. **Comptage Codex** → `iakatc-core/measure/codex.rs` (D3) : scan `CODEX_HOME`, agrégation des
   `token_count`, `cwd` via `session_meta`. **Parse défensif** + fixture réelle à confirmer.
4. **Contrats fichiers quota/config** → `iakatc-core/quota/store.rs` + `iakatc-core/quota/config.rs`
   (D4/D5) : lecture/validation `config.json` et `quota/*.json` (fichier malformé **ignoré +
   warning**, jamais de crash). Résolution `IAKATC_HOME`.
5. **Fusion hybride** → `iakatc-core/quota/merge.rs` (D5) : les 4 branches + étiquetage
   `confidence`/`source`/`notes`. Produit un `Reservoir` par `(account,provider,window)`.
6. **Agrégation deux axes** → `iakatc-core/aggregate.rs` (D6) : à partir des mesures Claude+Codex,
   produire les buckets projet×agent et ia×agent.
7. **Mapping contrat** → `iakatc-core/publish/contract.rs` : structures → **couples (topic-code,
   payload scalaire `{v,t}`)** exacts (un code par grandeur, `t` epoch s ; `v:null` si inconnu).
   Testé contre les exemples du contrat § 2/§ 3.
8. **Client MQTT** → `iakatc-daemon/mqtt.rs` : connexion rumqttc (env de config, § 6 du contrat),
   publish retained QoS 1, reconnexion/backoff, republication au retour en ligne.
9. **Boucle daemon** → `iakatc-daemon/main.rs` (D7) : tick, orchestration core → publish, gestion
   `current`/`last`, `meta/daemon/status`. Sous-commande `statusline-capture` (D4).
10. **Mocks** dans `specs/mock/` : JSONL Claude (coord + sidechain, multi-projets), rollout Codex
    **réel** (avec `token_count`), fichiers quota (frais/périmé/manquant/malformé), `config.json`
    avec/sans plafonds → jeu couvrant les 4 branches de fusion + les 2 axes.
11. **Tests** : parsing Claude (déjà portés) ; comptage Codex (fixture réelle) ; parsing
    config/quota + malformé ignoré ; 4 branches de fusion ; agrégation deux axes ; **topics/payloads
    exacts** vs contrat ; sérialisation `schema_version`/`ts` ; capture statusline (avec/sans
    `rate_limits`). Le publish MQTT réel est validé en test manuel (§ Vérification).
12. **README daemon** (`README.md` du crate) : variables d'env, exemple de config statusline,
    branchement broker, comportement hors-ligne, renvoi au `contrat-mqtt-conso.md`, limitations D5.

## Fichiers concernés

- `Cargo.toml` (workspace) ; `iakatc-core/Cargo.toml` ; `iakatc-daemon/Cargo.toml`.
- `iakatc-core/src/measure/claude.rs` — porté d'`economy.rs` (+ tests portés).
- `iakatc-core/src/measure/codex.rs` — comptage `token_count` (parse défensif).
- `iakatc-core/src/quota/store.rs` — lecture `quota/*.json` + `IAKATC_HOME`.
- `iakatc-core/src/quota/config.rs` — lecture/validation `config.json`.
- `iakatc-core/src/quota/merge.rs` — fusion hybride (D5) → `Reservoir`.
- `iakatc-core/src/aggregate.rs` — deux axes (D6).
- `iakatc-core/src/publish/contract.rs` — topics + payloads (mapping du contrat).
- `iakatc-daemon/src/mqtt.rs` — rumqttc, retained QoS 1, hors-ligne/backoff.
- `iakatc-daemon/src/main.rs` — boucle tick + sous-commande `statusline-capture`.
- `iakatc-daemon/README.md` — env, config statusline, hors-ligne, renvoi au contrat.
- `specs/mock/**` — fixtures JSONL Claude, rollout Codex réel, quota, config.

## Comportement attendu

Critères **observables et testables** :

- Sur un environnement **sans aucune donnée** (ni JSONL, ni quota), un tick **ne publie aucun code
  de conso/quota** et **ne lève aucune exception** ; les codes `meta/daemon/*` sont tout de même publiés.
- Un JSONL Claude avec un tour **coordinateur** et un tour **sous-agent** (`isSidechain:true`) sur
  le projet `P` produit des codes distincts, ex.
  `all/projets/agents/P/coordinator/conso/used_tokens/current` et `.../P/subagent/conso/used_tokens/current`,
  chacun avec un payload `{"v":<n>,"t":…}` conforme à la règle economy.rs.
- Les **mêmes** tokens ré-agrégés apparaissent sur l'axe `all/ia/agents/claude/coordinator/conso/used_tokens/current`.
- Un rollout Codex portant des `token_count` produit `all/ia/agents/codex/coordinator/conso/used_tokens/current`
  avec `v > 0` (fixture réelle).
- Fichier quota **frais** (`resets_at` futur, `captured_at` récent) → `.../quota/5h/confidence/current`
  = `{"v":"official",…}`, `.../source/current` = `{"v":"statusline",…}`, `.../used_pct/current` = la
  valeur du fichier, `.../remaining_pct/current` = `100 - used_pct`.
- `captured_at` au-delà du seuil mais `resets_at` futur → `confidence/current` = `{"v":"official_stale",…}`.
- Sans quota exploitable **mais** JSONL + plafond configuré → `confidence/current` =
  `{"v":"local_estimate",…}`, `used_pct/current` = `{"v":min(100,tokens/plafond*100),…}`.
- Sans quota **ni** plafond → `used_pct/current` = `{"v":null,…}`, `confidence/current` =
  `{"v":"none",…}`, `used_tokens/current` renseigné (`v` = valeur mesurée).
- Un `quota/*.json` **malformé** est **ignoré avec warning**, sans faire échouer le tick.
- Tout payload a la forme `{"v":<scalaire>,"t":<epoch_s>}` (jamais d'objet composite) ; les
  topics-codes émis correspondent **exactement** à `contrat-mqtt-conso.md` (test de non-régression
  sur les chaînes).
- **Retained** : après un publish, un `mosquitto_sub` neuf sur le topic-code **reçoit immédiatement**
  la dernière valeur (vérif manuelle).
- **Hors-ligne** : broker coupé, le daemon **tourne toujours**, journalise, et **republie** l'état
  courant à la reconnexion (vérif manuelle).
- `statusline-capture` : stdin **avec** `rate_limits` → écrit un fichier quota conforme au schéma
  D4 ; **sans** `rate_limits` → n'écrit rien et sort en **code 0** (statusline non cassée).

## Vérification

- [ ] `cargo check` / typecheck OK (workspace)
- [ ] `cargo clippy` (lint) OK
- [ ] `cargo test` vert : parsing Claude portés, comptage Codex (fixture réelle), config/quota +
      malformé, 4 branches de fusion, agrégation deux axes, topics/payloads exacts vs contrat,
      capture statusline
- [ ] Testé dans l'app réelle par le développeur : statusline branchée sur un vrai compte Pro/Max
      (fichier quota écrit) + tick sur de vrais JSONL Claude **et** un vrai rollout Codex +
      `mosquitto_sub` sur `iakatokencounter/#` montrant les retained + coupure/reprise du broker

## Hors scope

- **GUI tray / jauges** et leur rendu (instruction ultérieure).
- **Vendoring / abonnement dans IakaCockpit** (le daemon est construit ici ; l'intégration Cockpit
  est un chantier ultérieur).
- **Persistance CouchDB** des métriques (toucherait le pont iakaboxlogs) — MVP = MQTT retained seul.
- **Routage des conversations vers l'extérieur** / fusion des logs.
- **Toute modification du dépôt iakaboxlogs** (nom + code inchangés ; on publie sur son broker).
- **Providers autres que Claude Code + Codex** (Cursor, Gemini, Copilot…) et toute API usage/billing.
- **Résolution** des 4 pièges de D5 (ici seulement documentés + drapeaux `notes[]`).
- **Extraction d'`economy.rs` en crate partagé publié** (v0 = copie assumée, D2).

---

## Sources (veille)

- `economy.rs` (parsing usage Claude Code, tests) : `IakaCockpit/src-tauri/src/economy.rs`
- `codex.rs` (rollout Codex, `token_count` ignoré) : `IakaCockpit/src-tauri/src/codex.rs`
- Claude Code — statusline `rate_limits` (schéma, `resets_at` epoch s, pas d'ID compte) :
  https://code.claude.com/docs/en/statusline
- rumqttc — client MQTT Rust (retained, QoS, maintenu) : https://crates.io/crates/rumqttc
- MQTT retained — sémantique dernière valeur connue : https://www.hivemq.com/blog/mqtt-essentials-part-8-retained-messages/
- Contrat de sortie (topics/payloads/retained) : `specs/contrat-mqtt-conso.md`
