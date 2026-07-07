# Instruction : Collecteur de données (réservoirs par compte)

> Rédigé par Gandalf (P1 — cadrage). Consommé par Gimli comme instruction de travail.
> Première brique du produit : la couche qui alimentera ensuite le tray et l'analytics.

---

## Contexte

iakaTokenCounter est un moniteur de consommation IA multi-comptes en tray (cf.
`specs/PROJET.md`). Avant d'afficher la moindre jauge, il faut **une couche de collecte**
qui produit un objet normalisé « réservoirs par compte » — un `Reservoir[]` — à partir des
logs et signaux locaux, sans clé API.

Le décideur a verrouillé 4 décisions (journal `PROJET.md`, 2026-07-07) :
1. **Quota = mode hybride** : quota exact via le canal *statusline* de Claude Code quand
   disponible, estimation JSONL + plafond configuré sinon, avec **niveau de confiance affiché**.
2. **Réutiliser ccusage** (MIT, TS) comme lib de parsing usage (Claude Code + Codex).
3. **MVP = Claude Code + Codex CLI** uniquement.
4. **Techno = Tauri v2**.

Cette instruction **ferme le périmètre du seul collecteur**. Le tray, l'analytics UI, le
tokenizer de repli et les autres providers font l'objet d'instructions distinctes.

### Faits vérifiés (veille Gandalf, sources en bas)

- Le JSON *statusline* de Claude Code expose
  `rate_limits.five_hour.{used_percentage, resets_at}` et
  `rate_limits.seven_day.{used_percentage, resets_at}` (`used_percentage` 0–100,
  `resets_at` en epoch **secondes**). Ce bloc **n'est présent que** pour les abonnés
  Pro/Max, **après la première réponse API** de la session, et chaque fenêtre peut être
  absente indépendamment.
- **Le JSON statusline ne contient AUCUN identifiant de compte / e-mail** (champs : `model`,
  `workspace`, `cost`, `context_window`, `session_id`, `version`, `rate_limits`…). On ne
  peut donc pas distinguer nativement plusieurs comptes Claude via la statusline → limitation
  connue à contourner par étiquetage manuel (voir Décision).
- La statusline est un **script shell** que Claude Code exécute en lui passant ce JSON sur
  **stdin** ; le script écrit sa ligne d'affichage sur **stdout**. C'est le seul point
  d'injection pour capturer `rate_limits`.
- `ccusage/data-loader` (paquet `ccusage`, MIT, TS/ESM) expose des fonctions Node async :
  `loadSessionBlockData()` (blocs de **5 heures**), `loadWeeklyUsageData()`, `loadDailyUsageData()`,
  `loadSessionData()`… avec un objet d'options (répertoire de données Claude configurable).
- Le support **Codex** est fourni par un **paquet séparé `@ccusage/codex`** (beta) qui lit les
  JSONL de session sous `CODEX_HOME` (défaut `~/.codex`, liste séparée par virgules possible).
- Conséquence d'architecture : ccusage lit le disque via `node:fs`. **La webview Tauri ne peut
  pas exécuter ccusage** (pas de `fs` dans le navigateur). Le collecteur doit donc tourner dans
  un **runtime Node**, pas dans le front.

## Ce qui existe

| Élément | Où | État |
|---|---|---|
| Vision + décisions verrouillées | `specs/PROJET.md` | à jour |
| Dossier code | `src/` | vide (`.gitkeep`) |
| Dossier mocks | `specs/mock/` | vide (`.gitkeep`) |
| ccusage (Claude Code) | npm `ccusage` (`ccusage/data-loader`) | externe, MIT, à intégrer |
| ccusage (Codex) | npm `@ccusage/codex` | externe, beta, à intégrer |
| Canal quota exact | statusline Claude Code (`rate_limits`) | disponible, **non capturé** (aucun script) |
| Type `Reservoir` / `collectReservoirs()` | — | absent (objet de cette instruction) |

## Décision

### D1 — Le collecteur est un **module TS/Node autonome**, pas du code webview

**Retenu** : un module TypeScript exécuté en **runtime Node**, exposant `collectReservoirs()`
et un **point d'entrée CLI** qui imprime le `Reservoir[]` en JSON sur stdout.

**Pourquoi** : ccusage (`ccusage/data-loader`, `@ccusage/codex`) lit le disque via `node:fs`
et ne peut pas s'exécuter dans la webview Tauri. Le collecteur doit vivre côté Node.

**Écarté** :
- *Appeler ccusage depuis le front webview* : impossible (`node:fs` indisponible en webview).
- *Réimplémenter le parsing en Rust* : contredit la décision 2 (réutiliser ccusage).

**Frontière volontairement laissée ouverte (hors scope ici)** : *comment* Tauri invoque ce
module (sidecar binaire via `tauri-plugin-shell` / `externalBin`, ou spawn depuis Rust) est
une décision de l'instruction **tray**. Le collecteur est conçu **découplé et testable seul**
(entrée CLI → JSON stdout), pour ne pas préjuger de ce câblage. C'est le choix MVP le plus simple.

### D2 — Le script de capture statusline est **inclus dans cette instruction**

Sans lui, le mode exact est toujours vide. On inclut donc un **petit script de capture**
(Node) que l'utilisateur branche comme (ou dans) sa `statusLine` Claude Code : il lit le JSON
sur stdin, **persiste `rate_limits` dans un fichier**, puis ré-émet une ligne d'affichage
minimale sur stdout (pass-through, pour ne pas casser la statusline existante).

### D3 — Contrat du fichier de quota persistant

- **Racine configurable** : variable d'env `IAKATC_HOME`, défaut `~/.iakatokencounter/`.
- **Un fichier JSON par compte/provider** : `quota/<provider>.<account>.json`
  (ex. `quota/claude-code.default.json`). Un fichier par compte évite les races d'écriture
  entre sessions concurrentes ; le collecteur **globe** `quota/*.json`.
- **`<account>`** : la statusline ne fournit pas d'identifiant de compte. On l'obtient de la
  variable d'env `IAKATC_ACCOUNT_LABEL` (défaut `"default"`), positionnée par l'utilisateur
  dans sa config statusline. C'est l'étiquetage manuel multi-comptes du MVP.
- **Schéma d'un fichier de quota** (écrit par le script de capture) :
  ```json
  {
    "account": "default",
    "provider": "claude-code",
    "captured_at": 1738425600,
    "source_version": "2.1.90",
    "rate_limits": {
      "five_hour": { "used_percentage": 23.5, "resets_at": 1738425600 },
      "seven_day": { "used_percentage": 41.2, "resets_at": 1738857600 }
    }
  }
  ```
  `captured_at` = epoch **secondes** au moment de l'écriture (sert au calcul de fraîcheur).
  Une fenêtre absente dans la statusline est **omise** (pas écrite à `null`).

### D4 — Logique de fusion hybride (par couple compte × fenêtre)

Pour chaque `(account, provider, window ∈ {5h, 7d})`, choisir **une** valeur et **étiqueter**
la confiance :

1. **Exact frais** → `confidence: "official"`, `source: "statusline"` — si le fichier quota
   existe, contient la fenêtre, `resets_at > now`, ET `captured_at` plus récent que le seuil
   de fraîcheur (`FRESH_MAX_AGE_5H` défaut **20 min**, `FRESH_MAX_AGE_7D` défaut **6 h**).
2. **Exact périmé** → `confidence: "official_stale"`, `source: "statusline"` — fenêtre présente
   et `resets_at > now` mais `captured_at` au-delà du seuil (la valeur reste la dernière connue,
   mais l'utilisateur a pu consommer depuis).
3. **Estimation** → `confidence: "local_estimate"`, `source: "jsonl_estimate"` — sinon : on
   calcule à partir de ccusage (tokens cumulés sur la fenêtre) ÷ **plafond configuré**.
   `used_percentage = min(100, used_tokens / ceiling * 100)`.
4. **Aucune donnée** → `confidence: "none"`, `used_percentage: null` — ni exact utilisable, ni
   plafond configuré pour estimer (on peut tout de même remonter `used_tokens` en diagnostic).

Tous les seuils et plafonds sont **configurables** via `IAKATC_HOME/config.json` (voir D5).

### D5 — Contrat de configuration

Fichier `IAKATC_HOME/config.json`, tout optionnel (défauts raisonnables) :
```json
{
  "freshness": { "five_hour_seconds": 1200, "seven_day_seconds": 21600 },
  "ceilings": {
    "claude-code": { "default": { "five_hour_tokens": null, "seven_day_tokens": null } },
    "codex":       { "default": { "five_hour_tokens": null, "seven_day_tokens": null } }
  }
}
```
Les plafonds token exacts Pro/Max ne sont pas publiés par Anthropic → **laissés à `null` par
défaut** ; tant qu'un plafond n'est pas renseigné, l'estimation en pourcentage reste `null`
(confiance `"none"`) mais `used_tokens` est remonté. C'est un choix MVP assumé : ne pas inventer
de plafond faux.

### D6 — Type de sortie

```ts
type Provider   = "claude-code" | "codex";
type Window     = "5h" | "7d";
type Confidence = "official" | "official_stale" | "local_estimate" | "none";
type Source     = "statusline" | "jsonl_estimate";

interface Reservoir {
  account: string;
  provider: Provider;
  window: Window;
  used_percentage: number | null;      // 0..100, null si inconnu
  remaining_percentage: number | null; // 100 - used, null si inconnu
  resets_at: number | null;            // epoch secondes
  confidence: Confidence;
  source: Source;
  used_tokens: number | null;          // diagnostic (ccusage), null si indispo
  captured_at: number | null;          // pour sources exactes
  notes: string[];                     // drapeaux de limitations connues (voir D7)
}

declare function collectReservoirs(options?: {
  home?: string;                       // défaut IAKATC_HOME ou ~/.iakatokencounter
  dataSource?: CollectorDataSource;    // injection pour tests/mock (voir Étape 7)
}): Promise<Reservoir[]>;
```

### D7 — Pièges connus : **documentés comme limitations**, non résolus au MVP

Chaque `Reservoir` concerné porte un drapeau dans `notes[]` ; à documenter aussi dans le README
du module :
- `placeholder_input_tokens` — les JSONL peuvent journaliser des `input_tokens` placeholder/nuls
  → l'estimation **sous-compte**.
- `account_ambiguous` — la statusline n'a pas d'ID de compte ; les JSONL peuvent mélanger
  plusieurs comptes. Le MVP s'appuie sur l'étiquette manuelle → séparation par compte non fiable.
- `reset_anchor_first_prompt` — le `resets_at` d'une fenêtre 5h est ancré au premier prompt de la
  fenêtre ; les fenêtres d'estimation ccusage peuvent ne pas coïncider avec la vraie borne de reset.
- `tokenizer_shift` — un changement récent de tokenizer peut gonfler les comptes (~+30 %) →
  biais d'estimation tant que les plafonds ne sont pas recalibrés.

## Étapes d'implémentation

1. **Squelette du module** dans `src/collector/` (TS, ESM, cible Node ; runner de test type
   `vitest`). Déclarer les types de D6 dans `src/collector/types.ts`.
2. **Contrats fichiers** : implémenter lecture/validation du `config.json` (D5) et des fichiers
   `quota/*.json` (D3) avec **validation de schéma** (rejet propre des fichiers malformés →
   ignorés + warning, jamais de crash).
3. **Source estimation Claude Code** : brancher `ccusage/data-loader`
   (`loadSessionBlockData` pour 5h, `loadWeeklyUsageData` pour 7j) → tokens cumulés par fenêtre.
4. **Source estimation Codex** : brancher `@ccusage/codex` (lecture `CODEX_HOME`) → tokens
   cumulés par fenêtre 5h et 7j.
5. **Fusion hybride** : implémenter D4 (choix exact frais / exact périmé / estimation / aucune)
   et l'étiquetage `confidence`/`source`/`notes`.
6. **`collectReservoirs()`** : orchestrer sources + fusion → `Reservoir[]` (une entrée par
   compte × provider × fenêtre présents).
7. **Injection de source (`CollectorDataSource`)** : abstraire les accès disque/ccusage derrière
   une interface pour permettre de **substituer les mocks** en test sans vrais logs.
8. **Point d'entrée CLI** : `src/collector/cli.ts` qui appelle `collectReservoirs()` et imprime
   le JSON sur stdout (code retour 0 même si `Reservoir[]` est vide ; erreurs fatales sur stderr,
   code ≠ 0).
9. **Script de capture statusline** : `src/collector/statusline-capture.ts` (D2) — lit stdin,
   persiste `rate_limits` dans `quota/<provider>.<account>.json`, ré-émet une ligne minimale sur
   stdout. Doit rester **rapide** et **ne jamais planter** la statusline (échec silencieux + code 0).
10. **Mocks** dans `specs/mock/` : fixtures statusline-quota + sorties ccusage (Claude Code &
    Codex), fenêtres fraîches/périmées, cas manquants, cas malformés → jeu couvrant les 4 issues de D4.
11. **Tests** couvrant : parsing config/quota, chaque branche de fusion (official / official_stale /
    local_estimate / none), fenêtre absente, fichier malformé ignoré, multi-comptes par étiquette,
    présence des drapeaux `notes[]`.
12. **README du module** (`src/collector/README.md`) : contrats fichiers, variables d'env,
    exemple de config statusline, et les 4 limitations connues de D7.

## Fichiers concernés

- `src/collector/types.ts` — types `Reservoir`, `Provider`, `Window`, `Confidence`, `Source`, options.
- `src/collector/config.ts` — lecture/validation `config.json` + résolution `IAKATC_HOME`.
- `src/collector/quota-store.ts` — lecture/validation des `quota/*.json`.
- `src/collector/sources/claude-code.ts` — estimation via `ccusage/data-loader`.
- `src/collector/sources/codex.ts` — estimation via `@ccusage/codex`.
- `src/collector/merge.ts` — fusion hybride (D4) + étiquetage confiance/notes.
- `src/collector/collect.ts` — `collectReservoirs()`.
- `src/collector/cli.ts` — entrée CLI (JSON sur stdout).
- `src/collector/statusline-capture.ts` — script de capture statusline (D2).
- `src/collector/README.md` — contrats + limitations connues.
- `specs/mock/quota/*.json`, `specs/mock/ccusage/*.json` — fixtures.
- `src/collector/*.test.ts` — tests.
- `package.json` / config TS — dépendances `ccusage`, `@ccusage/codex`, runner de test.

## Comportement attendu

Critères **observables et testables** :

- `collectReservoirs()` retourne un `Reservoir[]` ; sur un environnement **sans aucune donnée**
  (ni quota, ni JSONL), il retourne `[]` **sans lever d'exception**.
- Avec un fichier quota **frais** (`resets_at` futur, `captured_at` récent), les réservoirs
  correspondants ont `confidence: "official"`, `source: "statusline"`, et `used_percentage` égal
  à la valeur du fichier.
- Avec un fichier quota dont `captured_at` dépasse le seuil de fraîcheur mais `resets_at` futur,
  la confiance est `"official_stale"`.
- Sans quota exploitable **mais** avec JSONL + plafond configuré, la confiance est
  `"local_estimate"`, `source: "jsonl_estimate"`, et `used_percentage = min(100, tokens/plafond*100)`.
- Sans quota exploitable **et** sans plafond, `used_percentage: null`, `confidence: "none"`,
  et `used_tokens` reflète la valeur ccusage.
- Deux étiquettes de compte distinctes (`IAKATC_ACCOUNT_LABEL`) produisent des réservoirs à
  `account` distincts ; le drapeau `account_ambiguous` est présent dans `notes[]`.
- Un fichier `quota/*.json` malformé est **ignoré avec warning**, sans faire échouer la collecte.
- La CLI imprime un JSON valide sur stdout et sort en code 0 sur un cas nominal (vérifiable par
  `node <cli> | jq` dans un environnement mocké).
- Le script de capture, alimenté par un JSON statusline **contenant** `rate_limits`, écrit un
  fichier quota conforme au schéma D3 ; alimenté par un JSON **sans** `rate_limits`, il n'écrit
  rien et sort en code 0 (statusline non cassée).
- `remaining_percentage` vaut `100 - used_percentage` quand `used_percentage` est non nul, sinon `null`.

## Vérification

- [ ] Typecheck OK
- [ ] Lint OK
- [ ] Tests ajoutés/à jour et verts (branches de fusion, malformé, multi-comptes, capture)
- [ ] Testé dans l'app réelle par le développeur (statusline branchée sur un vrai compte Pro/Max
      + collecte sur de vrais JSONL Claude Code et Codex)

## Hors scope

- **Tray / icône / jauges** et leur rendu (→ `feature-tray-jauges.md`).
- **App locale d'analytics** (historique, courbes) et le **store persistant** SQLite/JSON
  (→ `feature-app-analytics.md`).
- **Câblage Tauri** du collecteur (sidecar binaire vs spawn Rust, scheduling/polling) → décidé
  dans l'instruction tray.
- **Tokenizer de repli** interne (→ `feature-tokenizer.md`).
- **Autres providers** (Cursor, Gemini, Copilot…) et toute **API usage/billing** à clé.
- **Résolution** des 4 pièges de D7 (ici seulement documentés + drapeaux `notes[]`).
- **Recalibrage/valeurs réelles** des plafonds Pro/Max (config utilisateur, non fournie par le MVP).

---

## Sources (veille)

- Claude Code — Customize your status line (schéma JSON `rate_limits`, absence d'ID de compte) :
  https://code.claude.com/docs/en/statusline
- ccusage — Library usage / `ccusage/data-loader` (`loadSessionBlockData`, `loadWeeklyUsageData`) :
  https://ccusage.com/guide/library-usage
- ccusage — Codex Data Source (paquet `@ccusage/codex`, `CODEX_HOME`) : https://ccusage.com/guide/codex/
- ccusage — dépôt (MIT) : https://github.com/ryoppippi/ccusage
