# Instruction : Agents en cours — sprites de personas + parenté de délégation

> Rédigé par le cadrage (P1, Gandalf). Consommé par Gimli (P2).
> Besoin du décideur, verbatim : « je voudrais avoir dans le visuel de l app en taskbar des
> petit pixart representant les agents tournant: chaque agent avec entre parenthèses à sa
> droite les agents en délégation qu il a lancé et qui tournent » — précision MVP : « pour
> commencer les pixart seront la lettre du nom de l agent dans un carré arrondi de la couleur
> de sa pastille ».

---

## Contexte

L'app ne montre aujourd'hui que des **réservoirs de quota**. Elle ne dit pas **qui travaille
en ce moment** : combien d'agents tournent, lesquels, et qui a délégué à qui. Or c'est
précisément l'information que le décideur cherche d'un coup d'œil quand plusieurs sessions
Claude Code tournent en parallèle sur plusieurs projets.

Cette feature ajoute une **vue « agents en cours »** sur les deux surfaces déjà existantes :
un **compteur compact** dans l'icône de barre de menus, et le **détail avec la parenté** dans
la popover.

**Aucun blocage de source.** La question posée au cadrage (« le nom du persona est-il
récupérable ? ») est **tranchée par l'affirmative, sur preuve disque** : Claude Code écrit, à
côté de chaque transcript de sous-agent, un **sidecar `agent-<id>.meta.json`** qui porte
`agentType` (le nom du persona), `parentAgentId` et `spawnDepth`. Le repli « sprite générique »
n'est donc **pas** le cas nominal — il ne sert qu'aux types hors roster.

### Preuve disque (relevé du 2026-09-20, poste du décideur)

```
~/.claude/projects/<escaped>/
  <sid>.jsonl                            ← transcript du coordinateur de session
  <sid>/subagents/agent-<id>.jsonl       ← transcript du sous-agent  (→ liveness par mtime)
  <sid>/subagents/agent-<id>.meta.json   ← identité + parenté du sous-agent  (→ persona)
```

Contenu réel d'un `.meta.json`, profondeur 1 :

```json
{"agentType":"gandalf","description":"Cadrer la feature agents en cours",
 "toolUseId":"toolu_013irH9ZFSiCUTwm3qKK8YXT","spawnDepth":1,
 "requestShape":"background","requestNonInteractive":true}
```

Profondeur 2 — le champ **`parentAgentId`** apparaît :

```json
{"agentType":"loki","description":"Stand down Loki","toolUseId":"toolu_01ARZQBw…",
 "parentAgentId":"a32f41f2eb6ff10ea","spawnDepth":2,
 "requestShape":"background","requestNonInteractive":true}
```

Chiffres du relevé : **413** `subagents/agent-*.jsonl` ↔ **413** `.meta.json` (**appariement
1:1, aucun orphelin**) ; **466** `*.jsonl` au total sous `~/.claude/projects` ; **51** metas de
`spawnDepth ≥ 2` (la délégation imbriquée est donc un cas **réel**, pas théorique). Valeurs
d'`agentType` effectivement observées : `odin`, `aragorn`, `gandalf`, `gimli`, `legolas`,
`loki`, plus des types **hors roster** (`claude-code-guide`, `general-purpose`).

Le format est corroboré hors poste (structure `subagents/agent-<id>.jsonl` + sidecar
`.meta.json` portant `agentType` / `toolUseId` / `spawnDepth`) — cf. § Sources en fin
d'instruction. Il reste **non contractuel** : c'est un détail d'implémentation de Claude Code,
pas une API publiée (→ § Risques, R1).

## Ce qui existe

| Élément | Où | État |
|---|---|---|
| Marche récursive des transcripts Claude | `iakatc-core/src/measure/claude.rs` (`claude_transcript_files`) | **réutilisable tel quel** (ne ramasse que les `*.jsonl` ; les `.meta.json` sont ignorés — extension `json`) |
| Nom de projet depuis le `cwd` d'une ligne | `iakatc-core/src/measure/claude.rs` (`project_of`, `bucket_project`, `PORTFOLIO_ROOTS`) | **réutilisable tel quel** |
| Modèle d'agent binaire `Coordinator` / `Subagent` | `iakatc-core/src/measure/mod.rs` (`Agent`) | **NE PAS TOUCHER** — c'est l'axe du contrat MQTT de conso, sans rapport avec cette feature |
| Mémo de scan par `(mtime, taille)` | `iakatc-core/src/measure/cache.rs` | non réutilisé ici (il mémoïse un *contenu* ; ici on ne lit aucun contenu de transcript) |
| Icône tray composée (SVG → RGBA) | `src-tauri/src/icon.rs` — canvas **40 × 18**, logo 16×16 à (1,1), pistes 18×5 à x=20 | **étendue** (D5), zones existantes inchangées |
| Câblage tray (tooltip + `set_icon`) | `src-tauri/src/tray.rs` (`update_icon`, `update_tooltip`) | point d'accroche |
| État applicatif partagé | `src-tauri/src/state.rs` (`AppState`) | **+1 champ** (D8 — attention lot L1 en cours) |
| Thread d'échantillonnage détaché | `src-tauri/src/memory.rs` (`start_sampler`), lancé dans `lib.rs::setup` | **patron à recopier** pour le watcher |
| Popover (DOM + rendu) | `index.html`, `src/main.ts`, `src/render.ts`, `src/styles.css` | **+1 section** |
| Spec d'icône figée | `specs/instructions/feature-tray-visuals.md` D2 (40×18) / D3 (pire compte) | **étendue, pas amendée** (D5) |
| Harnais de test JS | *(aucun)* — `package.json` n'a ni script `test` ni `lint` | conditionne D9 : toute la logique décidable vit en **Rust** |
| Lot L1 « mémoire historique » non commité | `src-tauri/src/rollups.rs` (non suivi), `lib.rs` + `state.rs` modifiés | **chantier voisin** — points de contact traités en D8 |

## Décision

### D1 — Source d'identité : le sidecar `agent-<id>.meta.json`, et lui seul

Le persona d'un sous-agent est lu dans **`<sid>/subagents/agent-<id>.meta.json`**, champ
**`agentType`**. La parenté est lue dans le **même fichier** : `parentAgentId` (absent ⇒ le
parent est le **coordinateur de la session**) et `spawnDepth`.

**Écarté : le transcript du parent.** On aurait pu joindre l'appel de l'outil `Task`
(`input.subagent_type`, dans le `<sid>.jsonl` du parent) à son `tool_result` porteur de
l'`agentId`. Trois raisons de l'écarter : (a) il faut **lire le contenu** de transcripts qui
pèsent plusieurs Mo, alors que le sidecar fait ~200 octets ; (b) pour un `Task` **synchrone**,
le `tool_result` qui porte l'`agentId` n'est écrit qu'à la **fin** du sous-agent — donc la
jointure n'existe pas **pendant** qu'il tourne, ce qui est exactement le moment qui nous
intéresse ; (c) le sidecar donne en prime `parentAgentId`, qui rend la parenté imbriquée
**exacte** sans aucune heuristique.

**Écarté : le transcript de l'enfant.** Vérifié : `agent-<id>.jsonl` porte `agentId`,
`sessionId`, `isSidechain` — mais **aucun** champ de persona.

Défensif : un `.meta.json` absent, illisible ou au JSON invalide ⇒ l'agent est traité comme
**type inconnu** (sprite gris, D4), jamais ignoré ni source de panique. Un `*.jsonl` de
`subagents/` **sans** sidecar homonyme (ex. un futur `journal.jsonl` d'orchestration, signalé
en amont dans l'écosystème mais absent du poste) n'est **pas** un agent : il est ignoré.

### D2 — Liveness : écriture récente du transcript, **N = 90 s**

Un agent est « tournant » ssi le `mtime` de son transcript vérifie `now - mtime ≤ 90 s`.
Aucune détection de processus, aucun marqueur de fin de session (arbitrage décideur).

**Pourquoi 90 s** — c'est un arbitrage entre deux défauts symétriques. Claude Code n'écrit une
ligne qu'à chaque tour d'assistant ou résultat d'outil ; entre deux écritures, un agent
parfaitement vivant peut rester **silencieux** le temps d'un long raisonnement (souvent
20–60 s) ou d'un outil lent (build, suite de tests : facilement > 60 s). Trop court (30 s) →
les agents **clignotent**, disparaissent en plein travail, et l'indicateur devient un
mensonge par défaut. Trop long (120 s+) → les **fantômes** s'accumulent : un agent terminé
reste affiché deux minutes, et sur une salve de délégations on lit un effectif faux par excès.
90 s couvre largement le silence d'un tour + d'un outil courant, tout en bornant la durée de
vie d'un fantôme à une minute et demie. **Surchargeable** par `IAKATC_LIVENESS_SECS` (même
patron que les autres variables, `config.rs::from_env`), pour affiner en recette sans rebuild.

Un **coordinateur** est vivant si **son propre** transcript est frais **OU** s'il a au moins un
**descendant vivant** — sans quoi une session dont le coordinateur attend son sous-agent
afficherait des enfants orphelins.

### D3 — Rafraîchissement : watcher dédié, 5 s, **métadonnées seules**

Thread détaché lancé au `setup` (patron `memory::start_sampler`), tick **5 s**.

**Coût, chiffré.** Le watcher ne lit **jamais** un transcript : il fait un `read_dir` récursif
(déjà écrit : `claude_transcript_files`) puis un `metadata()` par fichier — **466 `stat()`** sur
le poste du décideur, de l'ordre de quelques millisecondes. Les seuls **contenus** lus sont
(a) les `.meta.json` des agents **vivants** (~200 octets, typiquement 0 à 5 par tick) et (b) la
**première ligne** du transcript de chaque session **vivante** (D7). Les 122 Mo / 701 Mo du
scan de mesure ne sont **jamais** retouchés : cette feature n'emprunte **rien** au chemin de
`measure::cache` et ne le dégrade pas.

L'instantané implémente `PartialEq` : l'événement n'est **émis que s'il a changé** (même
discipline que `ReservoirStore::apply_message`). Pas de re-render inutile toutes les 5 s.

### D4 — Roster et palette : la frame active, résolue en table figée

Ce projet n'a pas de pointeur `.iakaframe` → **frame default `iakaframe`**, personas lus dans
`~/work/iakaframe/library/personas/*.md` (champ `pastille` du frontmatter). Le mapping est
**codé en dur** dans une table Rust nommée : pas de lecture de `~/work/iakaframe` à l'exécution
(l'app doit rester **autonome**, cf. `CLAUDE.md` — « app autonome sans dépendance iaka »).

| `agentType` | Lettre | Pastille | Fond (hex) | Texte (hex) | Rôle |
|---|---|---|---|---|---|
| `odin` | **O** | 🟡 | `#FFD60A` | `#0B0D12` | portefeuille |
| `aragorn` | **A** | 🟠 | `#FF9F0A` | `#0B0D12` | coordination |
| `gandalf` | **G** | 🔵 | `#0A84FF` | `#FFFFFF` | cadrage (P1) |
| `gimli` | **G** | 🔴 | `#FF3B30` | `#FFFFFF` | réalisation (P2) |
| `legolas` | **L** | 🔴 | `#FF3B30` | `#FFFFFF` | qualité (P2/P3) |
| `helm` | **H** | 🟣 | `#BF5AF2` | `#FFFFFF` | production |
| `loki` | **L** | 🟠 | `#FF9F0A` | `#0B0D12` | design |
| `nathalie` | **N** | 🟠 | `#FF9F0A` | `#0B0D12` | documentation |
| `feanor` | **F** | 🟠 | `#FF9F0A` | `#0B0D12` | frame |
| *(tout autre)* | 1ʳᵉ lettre du type | ⚪ | `#6F6F78` | `#FFFFFF` | hors roster |

**Choix des hex.** Les pastilles sont des emojis, pas des couleurs : leur rendu varie d'une
police à l'autre. On ne les « échantillonne » donc pas — on les **projette sur la palette que
l'app utilise déjà** (couleurs système Apple, dont `#FF3B30` / `#FF9F0A` sont déjà les teintes
carburant de `icon.rs` et de `styles.css`). Une seule famille de couleurs dans toute l'app.

**Le discriminant est le couple (lettre, couleur), pas la lettre.** `gandalf`/`gimli` partagent
**G**, `legolas`/`loki` partagent **L** — mais les **neuf couples sont deux à deux distincts**
(cf. critère de vérification dédié). C'est une contrainte à **tester**, pas un hasard à subir :
si la frame gagne un persona, le test doit péter.

**Type hors roster** : fond gris + **première lettre du type** (`claude-code-guide` → `C` gris).
Le gris **est** le marqueur « pas un persona de la frame » ; l'infobulle donne le type complet.
On préfère la lettre réelle au `?` : plus informatif, aussi honnête.

### D5 — `gimli` et `legolas` : **couleur unique**, celle du frontmatter (🔴)

Arbitrage tranché : **pas** de couleur résolue par phase. Deux raisons, dont une dirimante.

1. **La phase n'est pas dans la donnée.** Le `.meta.json` porte `agentType`, `description`,
   `spawnDepth` — **rien** sur la phase servie. Résoudre la couleur par phase supposerait que
   l'agent la *publie* (nouveau canal à inventer) ou que l'app *devine* (mensonge). Le projet
   a déjà une règle sur ce point, et elle vaut ici : **jamais de faux plein**.
2. **Le frontmatter le dit lui-même.** `library/personas/legolas.md` déclare explicitement que
   `pastille: "🔴"` est la **pastille par défaut** et que la variation par phase est portée par
   le corps. Prendre la valeur du frontmatter, c'est prendre la valeur **canonique**, pas une
   approximation.

`gimli` = 🔴 `#FF3B30`, `legolas` = 🔴 `#FF3B30`, distingués par la lettre **G** / **L**.
La couleur résolue par phase est **hors périmètre** (§ Hors scope).

### D6 — Icône tray : **extension** du canvas, pas amendement de D2 tray-visuals

`feature-tray-visuals.md` **D2 reste intégralement en vigueur** : logo 16×16 à (1,1), pistes
18×5 (pilule r 2,5) aux origines (20,3) et (20,10), repli 1 barre à (20, 6.5), palette, mapping
`w = max(2, round(pct/100 × 18))`, traitements d'incertitude. **Aucune de ces cotes ne bouge.**
On **ajoute une zone à droite** :

- **Canvas : 40 × 18 → 54 × 18** (@2x **108 × 36**). Les 40 premiers pixels sont, au pixel près,
  l'icône d'aujourd'hui.
- **Séparateur** : filet `1 × 10` à (40.5, 4), `#8E8E93`, `opacity .5`.
- **Compteur** : carré arrondi `12 × 12`, `rx 3.5`, à (41, 3), fond `#6F6F78`, chiffre centré
  (`font-size 9.5`, `weight 700`, `#F3F3F6`). `> 9` → `9+` en `font-size 8`.
- **Zéro agent** : la zone est **vide** (rien de dessiné) ; le canvas **reste à 54** de large.
  On paie 14 px de barre de menus pour éviter que l'icône **change de largeur** et fasse sauter
  les items voisins toutes les quelques secondes. Point réversible en recette si le décideur
  préfère l'inverse.
- Le compteur est **neutre** (gris) **par construction** : c'est un effectif, pas un niveau. Il
  ne doit jamais se lire comme une teinte carburant.

Le commentaire D4 en tête de `src-tauri/src/tray.rs:1-3` (« pas de dessin fin dans l'icône […]
le détail vit dans la popover ») **reste vrai et doit être conservé** : un chiffre n'est pas du
dessin fin, et le détail (les sprites, la parenté) vit bien dans la popover. Gimli **complète**
ce commentaire pour mentionner la zone compteur, sans en changer l'esprit.

Un renvoi d'une ligne est ajouté dans `feature-tray-visuals.md` (§ D2) vers la présente
instruction, pour qu'on ne lise plus jamais « canvas 40×18 » sans voir l'extension.

### D7 — Popover : une ligne par session, parenthèses **imbriquées et exactes**

Nouvelle `<section id="agents">` **entre `#banners` et `<main id="reservoirs">`**, portant
l'attribut `hidden` **tant qu'aucun agent ne tourne** : popover strictement inchangée au repos.

Une **ligne par session vivante**, dans la forme demandée par le décideur :

```
iakaTokenCounter   O ( G  L ( N ) )
```

— sprite du coordinateur, puis, **s'il a des enfants vivants**, une **parenthèse littérale**
contenant leurs sprites ; récursivement pour les petits-enfants. `parentAgentId` donnant la
parenté **exacte à toute profondeur**, il n'y a **aucune** heuristique d'appariement ici.

- **Sprite** : carré arrondi `18 × 18`, `border-radius 5px`, fond + texte de la table D4, lettre
  centrée en gras. Rendu **DOM/CSS**, pas d'image.
- **Infobulle** d'un sprite : `<persona> — <description du meta> — actif il y a <n> s`.
- **Label de projet** : nom de projet obtenu en lisant la **première ligne** du transcript de
  session et en passant son `cwd` à `project_of` + `bucket_project` (**réemploi** de
  `iakatc-core`, donc même nom de projet que partout ailleurs dans l'app, seau « hors projet »
  compris). Lecture **bornée** (première ligne, plafond 1 MiB) et faite **uniquement** pour les
  sessions vivantes. Repli si illisible : le nom de dossier échappé, tel quel.
- **Parent terminé, enfant encore vivant** (cas réel des délégations `background`) : on remonte
  la chaîne `parentAgentId` — en lisant au besoin le `.meta.json` d'un ancêtre non vivant — et
  on rattache au **plus proche ancêtre vivant**, à défaut au coordinateur. L'infobulle mentionne
  alors le parent réel. Jamais d'agent orphelin non affiché.
- **Bornes d'affichage** : 6 sessions maximum (les plus récemment actives), 8 enfants par parent,
  au-delà un marqueur `+N`. La popover ne doit pas pouvoir grandir sans fin.
- **Tri** : sessions par activité décroissante ; enfants par **date de création** du transcript
  croissante (ordre de délégation), repli `agentId` lexicographique pour rester déterministe.

### D8 — Effectif compté et tuyauterie (anti-collision avec le lot L1)

**L'effectif** affiché dans l'icône = **tous** les agents vivants, **coordinateurs compris**
(un coordinateur *est* un agent — Odin, cf. `CLAUDE.md` global). Même ensemble que la popover :
un chiffre qui ne compte pas ce que la popover montre serait un piège.

Le lot L1 « mémoire historique » est **en travail non commité** (`rollups.rs` non suivi,
`lib.rs`/`state.rs` modifiés). Pour que les deux chantiers ne se marchent pas dessus, la
tuyauterie de cette feature est **strictement additive et disjointe** :

- **Nouveau module** `src-tauri/src/agents.rs` — aucun fichier existant réécrit.
- **`AppState`** : **un** champ ajouté, `pub running_agents: Mutex<AgentsSnapshot>`, placé
  **après** `rollups` (dernier champ actuel) — et l'initialisation correspondante en **dernière
  position** dans `Default`. Conflit textuel réduit à une ligne en fin de bloc.
- **`invoke_handler`** : commande `agents::get_running_agents` ajoutée **en dernier** dans la
  liste de `lib.rs`.
- **Événement dédié `tray://agents`** : on ne touche **pas** à `tray://state` ni à
  `mqtt_sub::push_state`. Le chemin quota et le chemin agents restent indépendants — une panne
  de broker n'éteint pas les sprites, et réciproquement.
- **Aucune modification** de `iakatc-core` : le module vit côté `src-tauri`, il ne consomme de
  `iakatc-core` que `claude_transcript_files`, `claude_projects_dir`, `project_of`,
  `bucket_project` (tous déjà publics ou `pub(crate)` à exposer sans changer de comportement).

### D9 — Toute la logique en Rust ; le TS ne fait que peindre

Le projet n'a **aucun harnais de test JS** (`package.json` : ni `test`, ni `lint`). Une règle de
placement en découle : le backend envoie un **modèle déjà résolu** (lettre, fond, texte,
infobulle, profondeur, ordre) et `render.ts` se contente de **peindre**. Zéro décision
— zéro seuil, zéro table de couleurs, zéro tri — côté TypeScript.

## Étapes d'implémentation

1. **`src-tauri/src/agents.rs` — roster (pur, testé).** Table `agentType → (lettre, fond, texte)`
   selon D4 ; fonction `sprite_for(agent_type: &str) -> Sprite` avec repli gris + première
   lettre. Tests : les 9 personas, un type hors roster, un type vide, et **l'unicité des couples
   (lettre, fond)** sur tout le roster.
2. **`agents.rs` — lecture du sidecar (pur, testé).** `AgentMeta { agent_type, description,
   parent_agent_id, spawn_depth }` désérialisé depuis le `.meta.json`. Tests : forme
   profondeur 1 (sans `parentAgentId`), forme profondeur 2 (avec), JSON invalide → `None`,
   champ manquant → valeur par défaut. **Défensif, jamais de panique.**
3. **`agents.rs` — découverte + liveness (testé sur dossier temporaire).** Depuis
   `claude_projects_dir()`, marche via `claude_transcript_files()` ; classement en
   `<sid>.jsonl` (coordinateur) vs `<sid>/subagents/agent-<id>.jsonl` (sous-agent, **exigeant**
   un sidecar homonyme) ; `metadata().modified()` → `is_live(now, mtime, n)` avec
   `now - mtime <= n`. Seuil `N = 90 s`, surchargeable `IAKATC_LIVENESS_SECS` via
   `config.rs::from_env`. Tests de bornes : 0 s, N-1, N, N+1.
4. **`agents.rs` — construction de l'arbre (pur, testé).** Regroupement par session ;
   rattachement par `parentAgentId` (absent ⇒ coordinateur) ; **remontée au plus proche ancêtre
   vivant** quand le parent direct est mort ; coordinateur vivant si transcript frais **ou**
   descendant vivant ; tris de D7 ; bornes 6/8 avec `+N`. Tests : parent + 2 enfants, enfant
   imbriqué (profondeur 2), parent mort/enfant vivant, coordinateur froid avec enfant vivant,
   session sans aucun agent vivant (absente du résultat).
5. **`agents.rs` — label de projet.** Lecture bornée de la première ligne du transcript de
   session (plafond 1 MiB) → `cwd` → `project_of` + `bucket_project`. Repli : nom de dossier
   échappé. Tests : `cwd` normal, racine de portefeuille (→ « hors projet »), fichier vide,
   première ligne non-JSON.
6. **`agents.rs` — instantané + commande + watcher.** `AgentsSnapshot` (`Serialize`, camelCase,
   `PartialEq`) ; `#[tauri::command] get_running_agents` ; `start_watcher(app)` — thread détaché,
   tick 5 s, patron `memory::start_sampler`, **émission de `tray://agents` seulement si
   l'instantané a changé**, et appel de la mise à jour d'icône.
7. **`src-tauri/src/state.rs` / `lib.rs` — câblage (D8).** Champ `running_agents` **en dernier**
   dans `AppState` + `Default` ; `mod agents;` ; commande **en dernier** dans `invoke_handler` ;
   `agents::start_watcher(handle.clone())` dans `setup`, **après** le sampler mémoire.
8. **`src-tauri/src/icon.rs` — extension du canvas (D6).** Largeur 40 → 54 (@2x 108×36),
   séparateur + pastille de compteur ; `render_icon` prend l'effectif en paramètre (ou une
   variante `render_icon_with_count`) ; `0` → zone vide. Tests purs : texte du badge
   (`0` → rien, `1`, `9`, `10` → `9+`, `99` → `9+`) et **largeur de canvas constante à 54**.
9. **`src-tauri/src/tray.rs` — câblage icône.** `update_icon` reçoit l'effectif ; compléter le
   commentaire D4 de tête pour décrire la zone compteur **sans contredire** « pas de dessin fin
   dans l'icône ».
10. **Front (peinture seule).** `index.html` : `<section id="agents" class="agents" hidden>`
    entre `#banners` et `#reservoirs`. `src/types.ts` : miroir de `AgentsSnapshot`.
    `src/render.ts` : `renderAgents(snap)` — sprites, parenthèses littérales, `+N`, infobulles ;
    `hidden` si vide. `src/main.ts` : `invoke("get_running_agents")` au démarrage +
    `listen("tray://agents")`. `src/styles.css` : `.agents`, `.sprite`, `.paren`, `.agents-proj`.
11. **Renvoi de spec.** Ajouter dans `specs/instructions/feature-tray-visuals.md`, sous D2, une
    ligne : « Canvas **étendu** à 54 × 18 par `feature-agents-en-cours.md` (D6) ; les cotes
    ci-dessous restent inchangées. »
12. **Vérification finale.** `cargo test`, `cargo clippy -D warnings`, `npm run typecheck`,
    `npm run build`, puis **essai réel** par le décideur avec une vraie salve de délégations.

## Fichiers concernés

- `src-tauri/src/agents.rs` — **nouveau** : roster, sidecar, liveness, arbre, snapshot, watcher, commande.
- `src-tauri/src/state.rs` — `AppState` : **+1 champ** `running_agents` (en dernier).
- `src-tauri/src/lib.rs` — `mod agents;`, commande en dernier dans `invoke_handler`, `start_watcher` au `setup`.
- `src-tauri/src/icon.rs` — canvas 54 × 18, séparateur + pastille de compteur.
- `src-tauri/src/tray.rs` — effectif passé à `update_icon`, commentaire de tête complété.
- `src-tauri/src/config.rs` — `IAKATC_LIVENESS_SECS` (défaut 90).
- `index.html` — `<section id="agents">`.
- `src/render.ts`, `src/types.ts`, `src/main.ts`, `src/styles.css` — rendu des sprites.
- `specs/instructions/feature-tray-visuals.md` — renvoi d'une ligne sous D2.

## Comportement attendu

- Quand **rien ne tourne** : l'icône est celle d'aujourd'hui (zone compteur vide) et la popover
  est **strictement inchangée** (section `hidden`).
- Quand des agents tournent : l'icône affiche l'**effectif** ; la popover montre, par session,
  `projet  <coordinateur> ( <enfants…> )` avec parenthèses **imbriquées** conformes à la
  délégation réelle.
- Un sprite = **une lettre dans un carré arrondi** à la couleur de la pastille du persona
  (table D4). Un type hors roster est **gris**, jamais déguisé en persona de la frame.
- Un agent disparaît de l'affichage **au plus 90 s** après sa dernière écriture ; il apparaît
  **au plus 5 s** après sa première.
- L'app ne relit **aucun** transcript pour ce calcul : `stat()` + sidecars de ~200 octets. Pas
  de dégradation mesurable de la conso CPU/IO au repos.
- Les **tests existants restent verts** ; aucun changement des chiffres de conso publiés ni du
  contrat MQTT.

## Risques

- **R1 — Le format `.meta.json` n'est pas contractuel.** C'est un détail interne de Claude Code,
  susceptible de bouger à une mise à jour. *Mitigation* : parsing **entièrement défensif**
  (champ absent / JSON invalide ⇒ type inconnu, sprite gris) ; la feature **se dégrade**, elle ne
  casse pas. Aucune autre partie de l'app ne dépend de ce module.
- **R2 — Collision de teintes avec la palette carburant.** `#FF3B30` (gimli/legolas) et
  `#FF9F0A` (aragorn/loki/nathalie/feanor) sont **aussi** les teintes « alerte » et « moyen » des
  réservoirs. *Mitigation* : les sprites sont des **carrés arrondis lettrés** dans une **section
  distincte**, jamais dans une piste de réservoir ; le compteur d'icône est **gris**, donc jamais
  lisible comme un niveau. À confirmer à l'œil en recette.
- **R3 — Fantômes et clignotements.** Le seuil de 90 s est un compromis, pas une vérité.
  *Mitigation* : `IAKATC_LIVENESS_SECS` permet d'ajuster en recette sans rebuild ; le réglage
  retenu sera figé après essai réel.
- **R4 — Conflit avec le lot L1 non commité.** *Mitigation* : D8 (module neuf, un seul champ
  ajouté en fin de `AppState`, commande en fin de `invoke_handler`, événement séparé). Si L1 est
  commité avant, le rebase est mécanique.
- **R5 — Largeur d'icône.** Passer de 40 à 54 px décale les items voisins de la barre de menus
  **une fois** (pas à chaque changement d'effectif, cf. D6). Décision assumée, réversible.

## Vérification

- [ ] `cargo test` vert, y compris les nouveaux tests d'`agents.rs` et d'`icon.rs`
- [ ] `cargo clippy --all-targets -- -D warnings` OK
- [ ] `npm run typecheck` OK et `npm run build` OK
- [ ] **Roster** : les 9 personas rendent la lettre et le fond de la table D4
- [ ] **Unicité** : un test parcourt le roster et échoue si deux personas partagent le **couple**
      (lettre, fond) — `gandalf`/`gimli` et `legolas`/`loki` inclus
- [ ] **Hors roster** : `claude-code-guide` → `C` sur `#6F6F78` ; `.meta.json` invalide → idem
- [ ] **Liveness** : `now - mtime` de 0 s, 89 s, 90 s → vivant ; 91 s → mort
- [ ] **Arbre** : parent + 2 enfants ; petit-enfant (profondeur 2) **imbriqué** ; parent mort →
      enfant rattaché au plus proche ancêtre vivant ; coordinateur froid + enfant vivant →
      session affichée ; session sans agent vivant → absente
- [ ] **Projet** : `cwd` normal → nom de projet ; racine de portefeuille → « hors projet » ;
      fichier illisible → repli sur le dossier échappé
- [ ] **Icône** : badge `0` → zone vide, `1`, `9`, `10` → `9+` ; **canvas 54 × 18 dans tous les cas**
- [ ] **Popover** : section `hidden` quand rien ne tourne ; aucune régression visuelle sur les
      cartes de réservoir
- [ ] **Émission** : deux ticks consécutifs sans changement n'émettent **qu'un seul** `tray://agents`
- [ ] **Essai réel** (décideur) : lancer 2–3 délégations, dont une imbriquée, et vérifier que
      l'icône et la popover disent la même chose que la réalité, apparition ≤ 5 s / disparition ≤ 90 s
- [ ] Les tests existants (conso, quota, rollups) **restent verts et inchangés**

## Hors scope

- **Le vrai pixel art.** Ce lot livre la lettre dans un carré arrondi — la forme MVP demandée.
  Les sprites dessinés sont une **itération ultérieure** ; la table D4 est justement le point
  d'extension (remplacer `Sprite::Letter` par `Sprite::PixelArt` sans toucher au reste).
- **Couleur résolue par phase** pour `gimli`/`legolas` (D5) : suppose un canal par lequel l'agent
  publie sa phase. À rouvrir si un tel canal existe un jour.
- **Codex.** Les rollouts `~/.codex/sessions/**` ne portent **ni** modèle de délégation **ni**
  champ de persona (`measure/codex.rs` modélise Codex en coordinateur unique). Rien à afficher :
  exclu par absence de donnée, pas par choix.
- **Historique des agents** (qui a tourné quand, durées, coût par persona) : c'est le terrain de
  la vue analytics et du lot « mémoire historique », pas celui-ci.
- **Interaction** : cliquer un sprite pour ouvrir la session/le transcript, notifications de fin
  d'agent, sons.
- **Détection de processus** et **marqueur de fin de session** : explicitement écartés par le
  décideur (D2).
- **Modification de `iakatc-core`**, du contrat MQTT, du daemon, du mémo de scan ou des chiffres
  de conso publiés.
- **Bundling CI** Windows/Linux, notarisation.

---

## Sources

Format des transcripts et du sidecar `.meta.json` — vérifié **directement sur le poste**
(relevé du 2026-09-20 détaillé au § Contexte) et corroboré par :

- [Create custom subagents — Claude Code Docs](https://code.claude.com/docs/en/sub-agents)
- [princess-pi/wtft, issue #137 — « every agent-\*.jsonl has a .meta.json with toolUseId, description and model »](https://github.com/princess-pi/wtft/issues/137)
- [kamp-us/phoenix, issue #8404 — transcript de sous-agent sous `<session>/subagents/agent-<id>.jsonl`](https://github.com/kamp-us/phoenix/issues/8404)
- [AgentWorkforce/relayhistory, issue #208 — `journal.jsonl` d'orchestration à côté des `agent-*.jsonl`](https://github.com/AgentWorkforce/relayhistory/issues/208)

Pastilles et roster : `~/work/iakaframe/library/personas/{odin,aragorn,gandalf,gimli,legolas,helm,loki,nathalie,feanor}.md`
(frontmatter `pastille`) ; la mention explicite « pastille par défaut » qui fonde D5 est dans
`legolas.md`.
