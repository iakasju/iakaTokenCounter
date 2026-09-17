# PROPOSITION : outil de statistiques riche (fenêtre analytics)

> ⚠️ **CE DOCUMENT N'EST PAS UNE INSTRUCTION VALIDÉE.** C'est une **proposition soumise à
> arbitrage**, rédigée par Gandalf (P1 — cadrage) à la demande du décideur. Aucun périmètre n'est
> fermé, aucun lot n'est engagé. Après arbitrage, le ou les lots retenus feront l'objet
> d'instructions fermées distinctes, au format habituel (`feature-*.md`), seules consommables par
> Gimli.
>
> Statut : **en attente d'arbitrage**. Date : 2026-09-17.
>
> **Révision 2 (même jour) — ampleur corrigée après contre-mesure d'Aragorn.** La première
> rédaction estimait l'inflation par doublons à « 3 à 5 fois » à partir d'**un seul fichier**, non
> représentatif, et en déduisait que le daemon montrait 29 % de la réalité. L'exécution sur le
> corpus entier donne **1,93 en moyenne** et **54 %**. Le diagnostic est inchangé et confirmé — deux
> défauts réels, indépendants — mais **le facteur de correction est ×1,85, pas ×3,4**. Les chiffres
> ci-dessous sont ceux de la contre-mesure.

---

## 1. Le besoin, reformulé

Le décideur demande : *« sur double clic sur la fenêtre des token, on ouvre une fenêtre de
statistique de l'utilisation des ia dans iaka. sur la base des infos dispo dans le mqtt, propose
moi un outil de stats très riche. »*

Reformulé en une phrase : **faire de la fenêtre analytics existante un tableau de bord qui
réponde aux questions de pilotage d'un portefeuille de projets consommant plusieurs comptes IA**
— qu'est-ce qui me coûte le plus, à quel rythme je consomme, est-ce que je tiens jusqu'au
rechargement, quel compte et quel modèle travaillent pour moi.

Deux précisions de cadrage, à poser d'emblée :

1. **« Très riche » n'est pas « beaucoup de graphiques ».** Ce document hiérarchise :
   indispensable / bon complément / luxe, et chiffre ce que coûte chaque strate.
2. **Ce document ne traite pas la forme.** Aucune maquette, aucune couleur, aucune mise en page :
   je cadre **ce qu'on montre et pourquoi**. La forme relève de 🎨 Loki, plus tard, sur la base
   de ce qui aura été arbitré ici. Deux exceptions assumées, où la donnée **contraint** la forme
   et où se taire serait malhonnête : l'échelle de représentation (§ 4.6) et le fait qu'un ratio
   constant ne se représente pas (§ 8).

---

## 2. Ce qui existe déjà — et ce à quoi je me raccroche

La fenêtre **existe** et est livrée (`open_analytics(provider, account)`,
`src-tauri/src/state.rs:260`). Elle contient déjà les jauges de quota 5 h / 7 j / 30 j
(`src/analytics.ts`), l'historique tokens/jour/projet + treemap + split coordinateur/sous-agent
(`src-tauri/src/history.rs:107`, `src/history.ts`) et la courbe de RAM système
(`src-tauri/src/memory.rs`). **Rien de tout cela n'est proposé ici comme une nouveauté.**

Son cadrage d'origine est `specs/instructions/feature-app-analytics.md`. **Je m'y raccroche, je ne
le contredis pas** :

| Décision d'origine | Statut aujourd'hui |
|---|---|
| **D1** — l'analytics est une vue de l'app tray, pas une app séparée | **Tient.** Rien dans cette proposition n'y touche. |
| **D2** — source = relecture disque, aucun store nouveau | **Sa clause d'échappement est déclenchée** — voir ci-dessous. |
| **D3** — `iatc-core` étendu en lecture seule uniquement | **Tient.** Tout ce que je propose reste read-only sur les logs. |
| **D4** — historique par **provider**, quota par **compte** (`account_ambiguous`) | **Tient, et je le confirme par mesure** (§ 4.5). |
| **D5** — visualisations maison SVG, aucune lib de charting | **Tient.** Aucune dépendance de charting proposée. |

**Le point unique où l'existant doit bouger — et il bouge selon sa propre règle.** D2 écartait un
store local en posant explicitement sa condition de réouverture : *« utile seulement le jour où
l'on voudra une granularité infra-journalière **ou une source qui s'efface du disque** ; ce n'est
pas le cas des JSONL »*. **C'est désormais le cas des JSONL** : Claude Code purge les transcripts
de plus de 30 jours au démarrage (`cleanupPeriodDays`, défaut 30 — source en fin de document),
`subagents/` compris. La prémisse factuelle de D2 est tombée ; sa clause d'échappement s'applique.
Je ne renverse pas D2, **je constate que la condition qu'il posait lui-même est remplie**.

**Intentions de `feature-app-analytics.md` restées non implémentées** (relevé demandé) :

- **Étape 9 — section README de la vue** (« ce que montre la vue, la source, la limitation par
  provider, le rafraîchissement manuel ») : `README.md` ne mentionne l'analytics qu'en trois
  lignes descriptives (`README.md:11`, `:74`, `:85`). La section détaillée n'existe pas.
- **D5 / split coordinateur-sous-agent — « donnée honnête via `sidechain` »** : le composant est
  bien là (`src/history.ts::historySplit`), mais il n'est pas honnête aujourd'hui, **non par
  erreur de rédaction mais parce que le périmètre de scan a changé sous lui** (§ 4.1). C'est le
  cœur de cette proposition.

---

## 3. En une page : ce que je demande au décideur de trancher

Pour qui ne lirait que cette section.

**Trois constats, mesurés, qui commandent tout le reste :**

1. **Le scan ne lit pas les transcripts de sous-agents** — 533 fichiers, 129 283 tours, **69,5 % de
   la consommation réelle**. Le split coordinateur/sous-agent affiche 100 / 0 alors que la vraie
   répartition est ≈ 30 / 70. (§ 4.1)
2. **Chaque appel API est compté 1,93 fois en moyenne** — un même `usage` est réécrit sur chaque
   bloc de contenu : 82 646 occurrences pour 42 865 appels réels. (§ 4.2)
3. **MQTT ne porte aucun historique** et n'en portera jamais : c'est une photographie de l'instant.
   Les statistiques ne peuvent pas reposer dessus. (§ 4.7)

**Effet net des deux premiers, mesuré :** la consommation réelle est de **9 195 840 499 tokens**
sur 45 jours ; l'outil en publie **≈ 4,96 milliards**. **Il montre 54 % de la réalité — facteur de
correction ×1,85.** (§ 4.1, § 4.2)

**Trois décisions attendues :**

| | Question | Ma recommandation |
|---|---|---|
| **A** | Sur quelle source repose l'outil ? | **Combinaison** : logs pour la consommation, quota MQTT **historisé localement** pour le réservoir et l'axe compte, rollups quotidiens pour la mémoire longue. (§ 5) |
| **B** | Que veut dire « consommé » ? | **Deux grandeurs nommées à l'écran** — *Travail* (hors cache réutilisé) et *Poids* (en argent, pondéré par le tarif du modèle). Jamais mélangées. (§ 6) |
| **C** | Par quoi commence-t-on ? | **L0, « Vérité des chiffres », 1,5 j.** Zéro nouvelle visualisation : il rend vraies les quatre qui existent. (§ 9) |

**Ce que je propose au total : 7 lots, 9,5 jour-homme**, dont un premier jalon de valeur à 5 j
(L0 + L1 + L2 : des chiffres justes, une mémoire qui se constitue, un coût en dollars). Le détail,
la hiérarchie des statistiques et les écarts assumés sont dans les sections suivantes.

---

## 4. Les faits mesurés — le socle de tout ce qui suit

Rien dans ce document ne repose sur une supposition. Les mesures viennent de deux campagnes sur
les **données réelles du décideur** : celles transmises par 👑 Aragorn (parsing complet de
`~/.claude/projects`) et les miennes (inspection ciblée des transcripts, lecture du code).

### 4.1 — Le scan ignore les sous-agents : 69,5 % de la consommation est invisible

**C'est le fait le plus lourd de cette proposition.** Il corrige une prémisse du brief.

Claude Code n'écrit plus les tours de sous-agents dans le transcript principal : il leur donne
**un fichier à part, dans un sous-répertoire** :

```
~/.claude/projects/<cwd-échappé>/<session-uuid>.jsonl          ← lu par le scan
~/.claude/projects/<cwd-échappé>/<session-uuid>/subagents/agent-<id>.jsonl   ← JAMAIS lu
```

Or `scan_projects_dir` (`iakatc-core/src/measure/claude.rs:108`) et `scan_projects_activity`
(`:231`) descendent d'**exactement deux niveaux** — dossier de projet, puis fichiers `.jsonl` —
et filtrent sur l'extension. Le répertoire `<session-uuid>/` n'a pas d'extension `.jsonl` : il est
**sauté**. Idem pour `scan_claude_measurements` (`:340`), donc pour ce que publie le daemon.

Mesuré sur le poste :

| Mesure | Valeur |
|---|---|
| Fichiers contenant des tours `isSidechain:true` | **533** — tous sous `subagents/` |
| Tours `isSidechain:true` au total | **129 283** |
| Tours `isSidechain:true` dans les transcripts principaux (`*/*.jsonl`) | **0** |

**Conséquence 1 — le split coordinateur/sous-agent n'est pas mort, il est aveugle.** Il affiche
100 % / 0 % non pas parce que le décideur ne délègue pas — il délègue massivement, 129 283 tours —
mais parce que le scan ne regarde pas là où sont les délégués. **Ce n'est pas une statistique à
écarter : c'est une statistique à réparer.** Je diverge ici explicitement de la lecture du brief,
sur la foi de la mesure.

**Conséquence 2 — l'énigme du `used_tokens` publié est close.** Le daemon publie `≈ 4,96 Md` pour
Claude là où un parsing récursif des logs donne `16,87 Md`. J'ai lu la règle exacte des deux
côtés :

- `fold_measure_line` (`claude.rs:275`) : `input = input_tokens + cache_creation + cache_read`,
  `output = output_tokens`, et `used_tokens = input + output` (`measure/mod.rs:63`).
- `aggregate::by_project_agent` / `by_provider_agent` (`aggregate.rs:14`, `:25`) : somme pure,
  **aucune déduplication, aucune pondération**.

Les deux comptages appliquent donc **la même règle** ; **seule l'assiette de fichiers diffère**.
La contre-mesure le démontre directement : en scannant **hors `subagents/`** et en gardant les
doublons — c'est-à-dire exactement ce que fait le daemon — on obtient **4 987 013 378** tokens,
contre `≈ 4,96 Md` publiés. **C'est la même valeur.** Le périmètre de scan explique l'écart en
totalité.

**Les quatre nombres qui comptent** (contre-mesure Aragorn, corpus entier, 45 jours) :

| Assiette | Tokens dédoublonnés | Appels API uniques |
|---|---|---|
| Hors `subagents/` (ce que le scan voit) | **2 800 536 564** | 7 510 |
| Dans `subagents/` (ce qu'il ignore) | **6 395 303 935** | 35 360 |
| **Total réel** | **9 195 840 499** | 42 870 \* |
| *Publié aujourd'hui par le daemon* | *≈ 4 960 000 000* (avec doublons, hors sous-agents) | — |

> \* Somme des deux assiettes. Le dénombrement global du corpus donne 42 865 identifiants
> distincts — **écart de 5 appels (0,01 %)**, sans incidence sur les volumes, à élucider en L0 si
> l'on veut un compte exact (probable effet de bord de comptage aux frontières de session).

**Le daemon mesure aujourd'hui 54 % de la consommation réelle** — facteur de correction **×1,85**.
Et la vraie répartition coordinateur / sous-agents est **≈ 30 / 70**, non 100 / 0.

> **Ce que j'avais écrit et qui était faux.** La première version de ce document annonçait 29 % et
> un facteur ×3,4, en prenant le total **brut avec doublons** (16,87 Md) pour la consommation
> réelle. C'était une erreur de référence : le brut n'est pas le réel. Les deux défauts existent
> bien, mais leur effet combiné est **deux fois moindre** que je ne l'avais estimé.

### 4.2 — Chaque appel API est compté 1,93 fois en moyenne

Un même message d'assistant est écrit sur **plusieurs lignes JSONL** (une par bloc de contenu :
texte, puis chaque `tool_use`), et **chaque ligne porte le même bloc `usage`, à l'identique**.

**Sur le corpus entier** (contre-mesure Aragorn) :

| Mesure | Valeur |
|---|---|
| Occurrences d'un bloc `usage` | **82 646** |
| `message.id` distincts (appels API réels) | **42 865** |
| **Facteur d'inflation moyen** | **1,93** |
| `message.id` partagés entre les deux assiettes | **0** — les deux défauts sont strictement indépendants, aucun double comptage croisé |

Le cas extrême que j'avais relevé par sondage — `…/00a28341-…/subagents/agent-a814e94e2420df709.jsonl`
— reste instructif sur le **mécanisme**, mais **n'est pas représentatif du volume** :

| `message.id` | Lignes portant ce `usage` |
|---|---|
| `msg_015BKwDmKLDDBP9BMaQVxCnD` | 2, 3, 5, 7, 8 — **5 fois**, `usage` strictement identique |
| `msg_01GGrojtQVBegBpk8ZZD4DDp` | 11, 12, 14, 16, 18 — **5 fois** |
| `msg_01KGUJDUz3xmSW8ctrh2ry1N` | 20, 21 — 2 fois |

Sur ce fichier : **15 enregistrements `usage` pour 5 appels API réels**, soit un facteur **3,0** —
presque le double de la moyenne du corpus. Une session très outillée multiplie les blocs
`tool_use`, donc les lignes ; une session conversationnelle en produit deux. **C'est la leçon de
méthode de cette révision : un fichier ne mesure pas un corpus.**

`fold_line`, `fold_activity_line` et `fold_measure_line` somment **toutes** les lignes : les trois
gonflent. L'écosystème a rencontré et corrigé exactement ce défaut (ccusage déduplique sur
`message.id`, en scopant par session — sources en fin de document).

**Conséquence : aucun chiffre affiché aujourd'hui n'est exploitable en valeur absolue.** Les
comparaisons relatives entre projets restent à peu près valides (le biais est diffus, il suit
l'intensité d'outillage), mais tout total, et *a fortiori* toute conversion en argent, est faux
d'un facteur de l'ordre de 2.

**Les deux défauts jouent en sens contraire, et ne se compensent pas** : le périmètre (§ 4.1) fait
**manquer 69,5 %**, les doublons font **ajouter ×1,93**. Leur combinaison donne un affichage à
**54 % de la réalité** — une coïncidence trompeuse, puisqu'elle ressemble à « un peu moins de la
moitié » sans qu'aucun des deux défauts ne soit d'une moitié. **Ils doivent être corrigés
ensemble** : appliquée seule, la déduplication ferait chuter les chiffres de moitié (alarme
injustifiée), et l'ouverture du périmètre seule les ferait tripler (euphorie injustifiée).

### 4.3 — Trois définitions incompatibles de « tokens consommés », dont deux dans la même fenêtre

Le code porte aujourd'hui **trois règles différentes**, chacune défendable, aucune affichée :

| Règle | Où | Formule | Traitement du `cache_read` |
|---|---|---|---|
| Économie (treemap) | `claude.rs::fold_line:66` | `input + cache_creation + cache_read`, `output` | **inclus** |
| Activité (timeline) | `claude.rs::fold_activity_line:196` | `input + output + cache_creation` | **exclu** |
| Mesure (MQTT) | `claude.rs::fold_measure_line:292` | `input` (caches inclus) `+ output` | **inclus** |

Avec les volumes réels — `cache_read = 16,2 Md` contre `620 M` de création, `44,8 M` de sortie et
`422 k` d'entrée fraîche — les deux premières règles diffèrent d'un facteur **≈ 25**.

**Autrement dit : la treemap et la timeline de la fenêtre analytics, côte à côte, sur les mêmes
logs, décrivent deux univers distants d'un facteur 25 — sans le dire.** Ce n'est pas un bug de
calcul : chaque règle est correcte pour son usage (l'activité veut mesurer le *travail neuf*,
l'économie veut mesurer la *facture*). Le défaut est de ne pas nommer laquelle on regarde.

**Il faut trancher et afficher.** Ma recommandation : **garder les deux grandeurs, nommées**, car
elles répondent à deux questions différentes du décideur — « combien ai-je travaillé » (hors cache
réutilisé) et « combien cela pèse » (tout compris) — et **ne jamais les mélanger dans une même
visualisation**. Le libellé doit être porté à l'écran, pas seulement en commentaire de code.

### 4.4 — Le modèle : la dimension la plus riche, aujourd'hui totalement inexploitée

`message.model` est présent sur chaque tour d'assistant. **Ni le contrat MQTT, ni `history.rs`,
ni aucune visualisation ne le remontent.** Répartition réelle sur 45 jours (mesure Aragorn) :

| Modèle | Entrée fraîche | Sortie | Cache read |
|---|---|---|---|
| `claude-opus-5` | 317 214 | 34 137 004 | **9 932 187 464** |
| `claude-sonnet-5` | 52 926 | 6 411 591 | **5 491 170 217** |
| `claude-fable-5-1` | 49 428 | 3 467 445 | 732 021 865 |
| `claude-opus-4-7`, `-4-8`, `claude-haiku-4-5` | résiduels | résiduels | résiduels |

**C'est la dimension qui transforme un comptage de tokens en une grandeur décidable.** Les tarifs
diffèrent d'un facteur 10 entre modèles, et d'un facteur 10 de plus entre un token d'entrée frais
et un token lu en cache. Sans l'axe modèle, « 9,2 milliards de tokens » ne veut rien dire ; avec
lui, cela devient un montant. Le backlog projet porte d'ailleurs déjà `feature-cost-estimate.md`
— cette proposition lui donne sa matière.

⚠️ **La ventilation par modèle ci-dessus est un comptage brut : périmètre complet, mais doublons
inclus.** Elle doit donc être ramenée au réel par le facteur du § 4.2 avant toute conversion en
argent. Le calcul en deux temps, avec les tarifs publics (entrée / sortie par million de tokens :
Opus 5 à 5 \$ / 25 \$, Sonnet 5 à 2 \$ / 10 \$, Fable 5.1 à 10 \$ / 50 \$ ; lecture de cache
≈ 0,1× l'entrée, création de cache ≈ 1,25× en TTL 5 min et ≈ 2× en TTL 1 h) :

| Poste (comptage brut) | Estimation brute |
|---|---|
| Cache read Opus 5 (9 932 M × 0,50 \$/M) | ≈ 4 970 \$ |
| Cache read Sonnet 5 (5 491 M × 0,20 \$/M) | ≈ 1 100 \$ |
| Sortie Opus 5 (34,1 M × 25 \$/M) | ≈ 850 \$ |
| Création de cache (621 M, modèle non ventilé) | ≈ 3 900 – 6 200 \$ |
| Reste (Fable, Sonnet sortie, entrée fraîche) | ≈ 440 \$ |
| **Sous-total brut, 45 jours** | ≈ 11 200 – 13 600 \$ |

Ramené au réel — `9 195 840 499 / 16 871 383 213 = 0,545`, à mix de modèles constant faute de
ventilation par assiette :

| | Tokens | Équivalent API, 45 jours |
|---|---|---|
| **Réalité estimée** (dédoublonnée, périmètre complet) | 9,20 Md | **≈ 6 100 – 7 400 \$** |
| **Ce que l'outil donne aujourd'hui** (avec doublons, sans sous-agents) | 4,99 Md | ≈ 3 300 – 4 000 \$ |

**Et c'est précisément pour cela que L0 passe en premier. Aujourd'hui, l'outil conduirait à une
facture d'environ 3 500 \$ là où la réalité est de l'ordre de 6 800 \$ : il en montre un peu plus
de la moitié.** Sur un plan d'abonnement, ce n'est pas une facture — c'est l'économie réalisée, et
elle est donc sous-estimée d'autant. Aucune « richesse » statistique ne rachète un chiffre faux au
facteur 2.

> Attention, tarifs non uniformes : la lecture de cache n'est pas partout à 0,1× de l'entrée
> (Fable 5.1 est à 0,25 \$/M). La table de prix doit porter des **taux explicites par modèle**
> (entrée, sortie, lecture de cache, création 5 min, création 1 h), jamais un multiplicateur
> global.

### 4.5 — Ce que les logs ne portent pas : l'identité du compte

Recherche exhaustive sur un transcript complet : `userID`, `accountUuid`, `organizationUuid` →
**zéro occurrence**. Seul `userType:"external"` est présent, sur chaque ligne, et ne distingue
rien. **La limitation `account_ambiguous` de D4 est donc confirmée par mesure, et elle est
définitive** tant que la source est le transcript : la consommation est attribuable au
**provider**, jamais au **compte**.

La question du décideur « quel compte est le plus sollicité ? » n'a donc **qu'une seule voie** :
l'axe **quota**, qui vient de la statusline et transite par MQTT — pas les logs. Ce point à lui
seul justifie une partie de l'arbitrage du § 5.

### 4.6 — Profondeur, volume, variance

| Fait | Valeur | Conséquence |
|---|---|---|
| Profondeur réelle des logs | **45 j** (2026-08-03 → 2026-09-17) | Ce que la vue appelle *all-time* est en fait **une fenêtre glissante**. |
| Purge automatique | `cleanupPeriodDays`, **défaut 30 j**, au démarrage, `subagents/` compris | **La source s'efface toute seule.** |
| Volume | 700 Mo, 584 fichiers ; 284 Mo en août → **416 Mo en septembre** | Relire tout le disque à chaque ouverture ne passera pas l'échelle. |
| Qualité | 82 624 lignes `usage`, **0 ligne illisible** | Le parsing défensif n'a rien à corriger de ce côté. |
| Variance journalière (cache read, 10 derniers jours) | 708 M, 359 M, 48 M, 656 M, **2 168 M**, 210 M, 57 M, 365 M, 176 M, 53 M | **Facteur 45** entre le jour creux et le jour chargé. |

Deux conséquences que je dois poser ici, parce qu'elles **contraignent la donnée** avant de
contraindre le graphisme :

1. **Une échelle linéaire écrase tout.** Avec un facteur 45, neuf jours sur dix seront des traits
   au ras de l'axe. Il faut soit une **échelle logarithmique**, soit une **normalisation**
   (pourcentage du plus haut, ou moyenne mobile). Le choix de la forme revient à Loki ; le fait
   qu'une échelle linéaire brute est disqualifiée est un **constat de donnée**.
2. **La profondeur affichable est plafonnée par la purge.** Toute promesse d'historique au-delà
   de 30 jours exige que l'application tienne **sa propre mémoire**. C'est l'objet du § 5.

### 4.7 — MQTT : ce qu'il porte, et ce qu'il ne portera jamais

Rappel des faits (mesures de la nuit, que je reprends sans les refaire) : 227 codes par tick
(60 s), tous *retained*, tous des **valeurs courantes** ; aucune série temporelle sur le fil ; le
broker ne persiste pas les *retained* sur disque (redémarrage = broker vide) ; depuis le lot B,
un code inchangé n'est plus republié, donc même la **fréquence** des messages ne renseigne plus
sur un rythme.

Je confirme par lecture du contrat (`iakatc-core/src/publish/contract.rs`) : chaque feuille est
un **scalaire `{v,t}`**, suffixe `current`. Il n'existe **aucun topic d'historique**, et les
`used_tokens` de conso sont des **cumuls depuis toujours**, non des deltas.

**Dit sans détour au décideur : « sur la base des infos dispo dans le mqtt » ne permet pas de
faire des statistiques.** MQTT est une photographie de l'instant, pas un film. Ce n'est pas un
défaut du transport : c'est son rôle. Ce qu'il apporte en revanche, et que les logs n'ont pas :
le **quota** (donc l'état du réservoir) et le **temps réel**.

### 4.8 — La granularité projet réellement atteignable

Le nom de projet n'est **pas** le nom du répertoire de `~/.claude/projects` : il est extrait du
**contenu**, ligne par ligne, par `project_of` (`claude.rs:40`) qui prend **le dernier segment du
`cwd`** porté par chaque enregistrement. D'où le paradoxe relevé par Aragorn : 6 répertoires
actifs en août, 12 en septembre, mais **44 projets** publiés.

C'est **plutôt une bonne nouvelle** : cette règle attribue correctement le travail d'un sous-agent
qui opère dans un autre répertoire que celui de la session. Mais elle a trois failles mesurables :

- **Seaux parasites.** Une session lancée depuis un répertoire de portefeuille produit un
  « projet » nommé `work` ou `Desktop` (les répertoires `-Users-sjupin-work` et
  `-Users-sjupin-Desktop-work` existent bel et bien). Ce ne sont pas des projets.
- **Collision de feuilles.** `/a/web` et `/b/web` fusionnent dans un unique projet `web`.
- **Chemins Windows historiques**, déjà traités défensivement par `project_of` (`claude.rs:381`).

**Granularité honnête atteignable : « dernier segment du `cwd` », ≈ 44 seaux, dont quelques-uns
parasites.** C'est suffisant pour un classement, insuffisant pour être affiché sans réserve. Ma
recommandation minimale : **conserver la règle**, afficher le **chemin complet en infobulle**, et
regrouper explicitement les racines de portefeuille sous un seau **« hors projet »**. Une table
d'alias est possible mais c'est de la configuration à maintenir — question ouverte au décideur
(§ 10).

---

## 5. Arbitrage n° 1 — sur quelle(s) source(s) repose l'outil ?

**C'est la question dont tout dépend.** Le décideur a dit « sur la base des infos dispo dans le
mqtt ». Ma réponse, sans détour : **MQTT seul ne permet aucune statistique**. Voici les quatre
options, ce que chacune permet et ce qu'elle interdit.

### Option A — MQTT seul

| | |
|---|---|
| **Permet** | L'état courant, en temps réel : quota restant par compte, cumuls de conso, santé du daemon. C'est exactement ce que le tray affiche déjà. |
| **Interdit** | **Toute série temporelle.** Aucun hier, aucune tendance, aucune vitesse, aucun classement sur une période. Un redémarrage remet même le broker à zéro. |
| **Verdict** | **Écartée comme socle.** Demander des statistiques à MQTT seul, c'est demander l'histoire d'un film à une photographie. |

### Option B — Fichiers de logs seuls (la voie actuelle de `history.rs`)

| | |
|---|---|
| **Permet** | La seule vraie profondeur temporelle : chaque événement est horodaté. Ventilation par jour, projet, agent, **modèle**, session. Rétroactif : le passé déjà écrit est exploitable dès le premier lancement. |
| **Interdit** | **Le quota** (il vient de la statusline, jamais des logs) — donc « vais-je tenir jusqu'au rechargement ? » reste sans réponse. **L'identité du compte** (§ 4.5). **La profondeur au-delà de la purge** : au-delà de 30 jours, la source n'existe plus (§ 4.6). |
| **Coût caché** | 700 Mo relus à chaque ouverture, et le volume croît vite (+416 Mo sur le seul mois de septembre). |
| **Verdict** | **Indispensable, mais insuffisante.** C'est la source de la *consommation*, pas celle du *réservoir*. |

### Option C — Historisation locale du flux MQTT

| | |
|---|---|
| **Permet** | Ce que les logs ne donnent pas : l'évolution du **quota** dans le temps — vitesse d'épuisement, projection jusqu'au rechargement, comparaison entre comptes. **Et c'est la seule voie vers l'axe « compte ».** |
| **Interdit** | Aucune rétroactivité : la série commence le jour où on l'allume, et ne se remplit que quand l'application tourne. |
| **Coût** | **Faible, et le motif existe déjà dans le projet** : `memory.rs` historise la RAM en JSONL borné, avec append, relecture triée, compaction atomique et rétention glissante — tout est écrit, testé, et réutilisable tel quel (`memory.rs:107`, `:119`, `:136`). Le tray est déjà abonné aux codes de quota (`mqtt_sub` → `state.rs::ReservoirStore`) : il n'y a **pas de nouvelle source à brancher**, juste un `append` sur un état déjà reçu. |
| **Verdict** | **Retenue, et à allumer le plus tôt possible** — sa valeur croît avec le temps de collecte. |

### Option D — Combinaison (ma recommandation)

**Trois flux, trois rôles distincts, aucun recouvrement :**

```
  logs Claude / Codex ──► conso : qui, quoi, quel modèle, quand      (profondeur ≤ 30 j)
          │                                    │
          └──► rollups quotidiens ─────────────┤  mémoire longue de l'app (au-delà de la purge)
                                               │
  MQTT (quota, statusline) ──► historisé ──────┤  vitesse d'épuisement, projection, axe COMPTE
                                               │
  MQTT (état courant) ────────────────────────►┘  temps réel, déjà en place, inchangé
```

**Pourquoi cette combinaison et pas autre chose :**

1. **Chaque source fait ce qu'elle seule sait faire.** Les logs portent la richesse (modèle,
   projet, agent) ; MQTT porte le réservoir et le compte ; l'historisation porte la durée.
2. **Elle n'historise que ce qui manque.** Je ne propose **pas** d'historiser les 176 codes de
   conso de MQTT : ce serait dupliquer, en moins bien et sans rétroactivité, ce que les logs
   portent déjà. **Seul le quota est historisé** — 35 codes, 5 réservoirs.
3. **Les rollups quotidiens règlent deux problèmes d'un coup** : la purge à 30 jours (§ 4.6) et le
   coût de relecture des 700 Mo. Un jour passé ne change plus : on le fige une fois, on ne le
   rescanne plus. Seule la fenêtre récente est relue.
4. **Elle respecte D2 dans sa lettre** : le store local n'est pas un « second magasin de calcul »
   — il ne recalcule rien, il **conserve** un résultat que la source va détruire.

**Ce que la combinaison ne rendra jamais possible, et qu'il faut dire au décideur :**

- Ventiler la **consommation** par compte (§ 4.5). Le quota, oui ; la conso, non. Jamais, tant
  que la source est le transcript.
- Reconstituer un **passé quota** antérieur à l'allumage de l'historisation. Ce qui n'a pas été
  capté est perdu.
- Descendre sous la **journée** pour les périodes anciennes (les rollups agrègent par jour ; la
  granularité fine ne survit que dans la fenêtre non purgée).

---

## 6. Arbitrage n° 2 — que veut dire « consommé » ?

Découle directement du § 4.3, et se tranche en un choix : **deux grandeurs nommées, jamais
mélangées.**

| Grandeur proposée | Formule | Répond à | Où elle sert |
|---|---|---|---|
| **Travail** | `entrée fraîche + création de cache + sortie` (hors cache réutilisé) | « Combien ai-je fait travailler l'IA ? » | Rythme quotidien, activité par projet, comparaisons |
| **Poids** | tous tokens confondus, **pondérés par le tarif du modèle** | « Combien cela pèse ? » | Coût équivalent API, classement des projets par coût |

**La recommandation est d'abandonner le « poids en tokens bruts »** (la règle actuelle de la
treemap) : un total qui additionne un token de cache read et un token de sortie additionne des
grandeurs dont les prix diffèrent d'un facteur 50. Ce total n'a aucun sens décisionnel. Dès que
l'axe modèle existe, **le poids s'exprime en argent, pas en tokens** — et le nombre brut reste
disponible en second rang, pour qui veut le voir.

**Dans tous les cas : la grandeur affichée doit être nommée à l'écran.** C'est la règle d'honnêteté
déjà appliquée par le bandeau de portée de D4 et par les badges `confidence` du contrat.

---

## 7. Les statistiques proposées, par ordre de nécessité

Légende des sources : **L** = logs · **Q** = quota historisé (MQTT) · **R** = rollups quotidiens.

### Strate 0 — Prérequis : sans quoi tout le reste ment

Ce ne sont pas des visualisations. C'est ce qui rend les visualisations existantes vraies.

| # | Prérequis | Source | Faisabilité | Valeur de décision |
|---|---|---|---|---|
| **P1** | Lire les transcripts `subagents/` | L | **Certaine.** Une descente de répertoire supplémentaire dans trois fonctions de scan déjà écrites ; le patron récursif existe déjà dans le projet (`codex.rs::walk_jsonl`). | **6 395 303 935 tokens** (69,5 % du réel) redeviennent visibles, et le split coordinateur/sous-agent devient ≈ 30 / 70. |
| **P2** | Dédupliquer par `message.id`, scopé session | L | **Certaine**, règle éprouvée par l'écosystème. Le champ `requestId` est présent en complément ; **aucun `message.id` n'est partagé entre les deux assiettes**, la déduplication est donc locale et sans effet de bord. | Supprime un facteur **×1,93** d'inflation. Combiné à P1, ramène l'affichage de 54 % à 100 % du réel. |
| **P3** | Nommer la grandeur affichée (§ 6) | — | **Certaine**, c'est une décision et un libellé. | Rend les chiffres rapprochables de quelque chose. Sans lui, deux visualisations voisines se contredisent en silence. |

### Strate 1 — Indispensable : ce qui fait décider

**S1 — Coût équivalent API, global et par projet** · source **L** · faisabilité **haute**
L'axe `message.model` existe dans les logs et n'est lu par personne (§ 4.4). Avec une table de
tarifs par modèle (entrée, sortie, cache read, cache write 5 min / 1 h), chaque tour devient un
montant. **Valeur** : c'est la seule grandeur qui répond vraiment à « qu'est-ce qui me coûte le
plus » — un classement en tokens bruts mélange des choses à 0,50 \$ et à 25 \$ le million.
Effet de bord précieux : le montant « que mon abonnement m'évite de payer » est un chiffre que le
décideur peut opposer au prix de son plan. **Dépend de P1 + P2**, sans quoi il sous-estime la
réalité d'un facteur ≈ 1,85 (§ 4.4).

**S2 — Vitesse d'épuisement du quota et projection jusqu'au rechargement** · source **Q** ·
faisabilité **haute, mais valeur différée**
Courbe du `remaining_pct` par compte et par fenêtre (5 h / 7 j / 30 j), avec la pente récente
projetée jusqu'à `resets_at` — déjà publié par le contrat. **Valeur** : c'est la réponse directe à
« vais-je tenir jusqu'au rechargement ? », la question la plus opérationnelle de la liste, et
celle à laquelle *rien* ne répond aujourd'hui (la jauge dit où on en est, pas où l'on va).
**Réserve honnête** : la courbe est vide le premier jour et ne vaut qu'après quelques jours de
collecte. **C'est l'argument pour l'allumer tôt, même si on l'affiche tard.**

**S3 — Classement des projets sur une période choisie (7 j / 30 j / tout)** · source **L + R** ·
faisabilité **haute**
Le treemap actuel est *all-time* : les gros projets anciens masquent l'activité réelle du moment.
**Valeur** : « où est passé mon mois » est une question d'arbitrage ; « où est passé mon année »
est une question d'archive. Le sélecteur de période transforme une curiosité en outil de pilotage.

**S4 — Rythme quotidien global, tous projets** · source **L + R** · faisabilité **haute**
Une seule courbe : tokens (grandeur *Travail*, § 6) par jour, tous projets confondus, avec moyenne
mobile 7 jours. **Valeur** : donne la « vitesse de croisière » et situe la journée en cours par
rapport à elle. La timeline actuelle, une ligne par projet en bulles, ne permet pas de lire un
rythme d'ensemble. **Contrainte de donnée** : facteur 45 entre jour creux et jour chargé (§ 4.6) —
échelle logarithmique ou normalisée obligatoire.

### Strate 2 — Bon complément : ce qui affine le pilotage

**S5 — Ventilation par modèle (volume et coût), et croisement projet × modèle** · source **L** ·
faisabilité **haute** (acquise avec S1)
**Valeur** : révèle les arbitrages invisibles — un projet qui consomme de l'Opus là où du Sonnet
suffirait, ou l'inverse. Aujourd'hui, 9,9 Md de cache read en Opus 5 contre 5,5 Md en Sonnet 5 :
le décideur ne dispose d'aucun moyen de voir cette répartition, ni de savoir quels projets la
produisent.

**S6 — Coordinateur vs sous-agents, enfin vrai** · source **L** · faisabilité **certaine** (acquise
avec P1)
**Valeur** : le décideur pilote une équipe d'agents. Savoir que **69,5 % de sa consommation part en
délégation** — 6,40 Md de tokens sur 35 360 appels, contre 2,80 Md sur 7 510 pour le coordinateur —
et sur quels projets, est un fait de pilotage de premier ordre. **La visualisation existe déjà**
(`historySplit`) : P1 suffit à la rendre vraie, aucun développement d'affichage.

**S7 — Consommation par persona d'agent** (gandalf, gimli, legolas, loki, nathalie…) · source **L**
· faisabilité **moyenne**
Le lien existe et je l'ai vérifié : le transcript principal porte l'appel `Task` avec
`subagent_type` (le nom de la persona) et, sur l'enregistrement de résultat,
`toolUseResult.agentId` — qui est exactement le nom du fichier `subagents/agent-<id>.jsonl`. La
jointure `persona → agentId → fichier → tokens` est donc reconstructible.
**Valeur** : propre à iakaframe, et forte — « quel membre de mon équipe me coûte le plus ».
**Risque** : ce format n'est pas documenté par l'éditeur et peut changer ; la jointure doit
**dégrader proprement** (persona inconnue → seau « non attribué »), jamais faire tomber la vue.

**S8 — Comparaison de comptes** · source **Q** · faisabilité **haute** (acquise avec S2)
Les courbes de quota des comptes côte à côte. **Valeur** : c'est la **seule** réponse possible à
« quel compte est le plus sollicité » (§ 4.5). Sans l'historisation, cette question reste sans
réponse pour toujours.

**S9 — Décomposition du coût : lecture de cache / création de cache / sortie** · source **L** ·
faisabilité **haute** (acquise avec S1)
**En argent, pas en ratio** — voir § 8 pour pourquoi le ratio est disqualifié. **Valeur** : montre
où part réellement la dépense. Avec 621 M de tokens de création de cache à un tarif supérieur à
l'entrée fraîche, la création de cache pèse probablement plus lourd que la sortie du modèle — un
fait totalement invisible aujourd'hui, et actionnable (il dépend du découpage des sessions).

### Strate 3 — Luxe : ce qu'on ajoute quand tout le reste est en place

| # | Statistique | Source | Faisabilité | Valeur |
|---|---|---|---|---|
| **S10** | Heatmap heure × jour de l'activité | L | Haute (`timestamp` à la seconde) | Faible en décision, forte en connaissance de soi : quand je travaille vraiment. |
| **S11** | Part de réflexion (`output_tokens_details.thinking_tokens`) dans la sortie | L | Haute (champ présent) | Curiosité instrumentée ; peut éclairer un réglage d'effort. |
| **S12** | Top des sessions les plus lourdes | L | Haute | Permet de retrouver *la* session qui a mangé la journée à 2 168 M. |
| **S13** | Appels d'outils serveur (`server_tool_use` : web search / web fetch) | L | Haute | Anecdotique aujourd'hui (compteurs à 0 sur l'échantillon). |
| **S14** | Corrélation RAM ↔ activité IA | L + `memory.rs` | Moyenne (deux axes de temps à aligner) | Confort d'observabilité ; aucune décision n'en dépend. |

---

## 8. Ce que j'écarte, et pourquoi

### 8.1 — Le taux de réutilisation du cache : **écarté, mesuré mort-né**

Le rapport `cache_read / (entrée fraîche + cache_read)` vaut **100,0 %** sur les données réelles :
16 205 264 234 contre 422 422 d'entrée fraîche. Ce n'est pas un hasard conjoncturel mais un
invariant de fonctionnement : le prompt entier est mis en cache, l'entrée fraîche se réduit à
quelques tokens par requête. **Un indicateur qui affichera 100 % à vie n'informe de rien et ne se
représente pas** — une jauge qui ne bouge jamais est un bandeau décoratif.

**Ce qui le remplace : S9**, la décomposition du coût cache en **argent** (lecture ≈ 0,1× l'entrée,
création ≈ 1,25× à 2×). Le volume absolu et son prix varient, eux, et se pilotent.

### 8.2 — Ce que je **n'écarte pas**, contrairement à ce qui m'était suggéré : le split coordinateur / sous-agent

Il m'était demandé de le ranger ici, au même titre que le taux de cache, comme « 100 / 0 à vie ».
**La mesure dit le contraire, et je dois le dire.**

Le taux de cache est à 100 % *parce que la réalité est à 100 %*. Le split est à 100 / 0 *parce
qu'on ne lit pas les fichiers où se trouve l'autre part* : **533 fichiers, 129 283 tours
`isSidechain:true`, aucun lu** (§ 4.1). Ce n'est pas le même diagnostic, et cela n'appelle pas le
même traitement :

| | Taux de cache | Split coord / sous-agent |
|---|---|---|
| Valeur affichée | 100 % | 100 / 0 |
| Cause | La réalité **est** à 100 % | Le scan **ne voit pas** 6,40 Md de tokens |
| Remède | Aucun — la grandeur n'a pas d'information | **Une descente de répertoire** (P1) |
| Après remède | Toujours 100 % | **≈ 30 / 70**, et une des statistiques les plus parlantes pour qui pilote une équipe d'agents |

**Je maintiens donc S6.** La contre-mesure sur le corpus entier a depuis confirmé la déduction :
2 800 536 564 tokens côté coordinateur, 6 395 303 935 côté sous-agents. Ce n'était pas une
hypothèse à valider en L0 — c'est un fait établi avant l'arbitrage.

### 8.3 — Les autres écarts

| Écarté | Pourquoi |
|---|---|
| **Ventilation de la consommation par compte** | Aucun identifiant de compte dans les transcripts (§ 4.5, vérifié). Inventer une attribution serait mentir ; D4 l'a déjà tranché. Le quota, lui, reste par compte. |
| **Coût « réel » en euros** | Le décideur est sur abonnement : son coût marginal est nul. Seul un **équivalent API** a du sens, et il doit être étiqueté comme tel. Afficher « vous avez dépensé 6 800 \$ » serait faux ; « votre abonnement vous en a évité 6 800 » est juste. |
| **Historisation des 176 codes de conso MQTT** | Doublon appauvri des logs : sans rétroactivité, sans modèle, sans jour. On n'historise que ce que les logs n'ont pas — le quota (§ 5). |
| **Prévision statistique élaborée** (régression, saisonnalité, ML) | Sur-ingénierie. Une extrapolation linéaire de la pente récente jusqu'à `resets_at` répond à la question posée ; au-delà, on habille du bruit (facteur 45 entre jours). |
| **Alertes / notifications de seuil** | Ce n'est pas de l'analytics : c'est le tray. Sujet légitime, périmètre différent, à cadrer à part. |
| **Export CSV / PDF, partage** | Non demandé, déjà hors scope de `feature-app-analytics.md`. Trivial à ajouter plus tard si le besoin apparaît. |
| **Comptage de tokens par tokenizer local** | La brique `feature-tokenizer.md` du backlog n'a aucune utilité ici : les logs portent l'`usage` réel, mesuré par le fournisseur. Le tokenizer reste un repli pour les sources qui ne donnent pas l'usage. |
| **Nouvelle bibliothèque de graphiques** | D5 l'a tranché : visualisations maison en SVG. Rien dans cette proposition ne l'exige. |

---

## 9. Découpage en lots et estimation

**Chaque lot est livrable seul et apporte de la valeur seul.** L'estimation est un **ordre de
grandeur assumé et révisable**, pas un engagement ferme ; elle sera confrontée au temps réel à la
clôture de chaque lot.

| Lot | Contenu | Jour-homme | Complexité / risque |
|---|---|---|---|
| **L0** | **Vérité des chiffres** — P1 (lire `subagents/`) + P2 (déduplication `message.id`) + P3 (nommer la grandeur) + mesure consignée de l'écart avant/après | **1,5 j** | Moyenne. Risque : les chiffres affichés vont **bouger fortement** — il faut le dire à l'écran, sinon le décideur croira à une régression. |
| **L1** | **Mémoire de l'app** — historisation du quota (patron `memory.rs`) + rollups quotidiens de conso, avec rétention et compaction | **2 j** | Moyenne. Le patron d'écriture est déjà écrit et testé ; l'inconnue est le réglage cadence / rétention. |
| **L2** | **Coût équivalent API** — axe `message.model` de bout en bout + table de tarifs par modèle + S1, S5, S9 | **1,5 j** | Moyenne. Risque : **maintenance de la table de prix** (tarifs cache non uniformes selon les modèles). |
| **L3** | **Tableau de bord périodisé** — sélecteur 7 j / 30 j / tout + S3 + S4, échelle adaptée à la variance | **1,5 j** | Faible. Dépend de L0 pour les valeurs et de L1 pour la profondeur. |
| **L4** | **Quota dans le temps** — S2 (vitesse, projection) + S8 (comparaison de comptes) | **1 j** | Faible techniquement. Ne devient visible qu'après quelques jours de collecte par L1. |
| **L5** | **Axe persona** — S7, jointure `Task` → `agentId` → transcript de sous-agent | **1 j** | **Moyenne-haute.** Format non documenté par l'éditeur ; exige une dégradation propre. |
| **L6** | **Luxe** — S10 à S14, au choix du décideur | **1 j** | Faible. Sécable à volonté. |
| **L7** *(conditionnel)* | **Index incrémental du scan** — n'ouvrir que si L0 mesure une ouverture de vue > 2 s | **1 j** | Moyenne. À décider sur mesure, pas par anticipation. |

**Total : 9,5 j** (10,5 j avec le lot conditionnel).

### Le premier lot, et pourquoi c'est celui-là

**L0 (1,5 j) — « Vérité des chiffres ».** Il n'ajoute **aucune** visualisation : il rend vraies les
quatre qui existent déjà. C'est le seul lot que je recommande sans réserve, quel que soit
l'arbitrage sur le reste, pour une raison simple : **aujourd'hui, l'outil affiche 54 % de la
réalité.** Ajouter des statistiques au-dessus de cela, c'est multiplier les décimales d'un chiffre
faux au facteur 2.

Trois livrables observables, seuls, sans rien d'autre, et **chacun vérifiable contre une valeur
attendue connue** (c'est ce qui rend ce lot testable avant même d'être vu) :
1. le split coordinateur / sous-agent cesse d'afficher 100 / 0 et affiche **≈ 30 / 70** ;
2. le total Claude *all-time* passe de `≈ 4,96 Md` à **9 195 840 499**, décomposable en
   2 800 536 564 + 6 395 303 935 ;
3. la fenêtre dit enfin quelle grandeur elle montre.

### Ordre recommandé et logique de séquence

**L0 → L1 → L2 → L3 → L4 → L5 → L6.**

- **L0 d'abord** : tout le reste s'appuie dessus.
- **L1 très tôt, même si son affichage vient plus tard (L4).** C'est le seul lot dont la valeur
  **dépend du temps écoulé depuis sa mise en route** : chaque semaine de retard est une semaine
  d'histoire perdue, et la purge à 30 jours rogne en permanence par l'autre bout. On plante
  l'arbre avant d'avoir faim.
- **L2 ensuite** : c'est le lot le plus spectaculaire pour le décideur (les tokens deviennent de
  l'argent) et il ne demande rien d'autre que L0.
- **L3 à L6** : confort et finesse, dans l'ordre que le décideur préférera.

**Premier jalon de valeur : L0 + L1 + L2 = 5 j.** À ce stade, la fenêtre dit la vérité, mesure en
argent, et a commencé à se constituer une mémoire.

### Inconnues susceptibles de faire glisser l'estimation

1. ~~La confirmation de l'écart de comptage.~~ **Levée** : la contre-mesure a reproduit à
   l'identique le `used_tokens` du daemon hors `subagents/` (4 987 013 378 contre ≈ 4,96 Md) et
   établi le total réel à 9 195 840 499. L0 démarre avec ses valeurs cibles connues.
2. **Le coût de relecture après P1.** Ajouter 533 fichiers à un scan déjà à 700 Mo peut rendre
   l'ouverture de la vue inconfortable — d'où L7, conditionnel et mesuré, jamais anticipé.
   **C'est désormais l'inconnue n° 1 de L0.**
3. **La stabilité du format de Claude Code** (jointure persona de L5, chemin `subagents/` de P1).
   Non documenté, susceptible de changer sans préavis.
4. **La table de tarifs** (L2) : périmètre exact (quels modèles, quels taux de cache) et qui la
   maintient.
5. **Codex** : tout ce qui précède vise Claude. L'étendre à Codex n'est pas chiffré ici (question
   ouverte n° 5).

---

## 10. Questions ouvertes au décideur

Sept questions. Les trois premières sont structurantes — elles changent le contenu des lots. Les
autres peuvent se trancher plus tard, mais coûtent moins cher tranchées maintenant.

**Q1 — Valide-t-on l'arbitrage des sources du § 5 (option D, combinaison) ?**
En particulier : accepte-t-on que l'application tienne **sa propre mémoire** (historisation du
quota + rollups quotidiens), c'est-à-dire qu'on rouvre D2 de `feature-app-analytics.md` — par la
clause d'échappement que D2 avait lui-même posée ? Si non, S2, S8, et toute profondeur au-delà de
30 jours sont définitivement hors d'atteinte.

**Q2 — La fenêtre reste-t-elle « par provider », ou devient-elle un tableau de bord de
portefeuille ?**
Elle s'ouvre aujourd'hui sur un double-clic **sur un compte**, et montre l'historique **d'un
provider** (D4). La demande parle de « l'utilisation des ia dans iaka », au pluriel. Deux réponses
possibles, et elles n'ont pas le même coût : (a) on garde une vue par provider, on l'enrichit ;
(b) on en fait une vue portefeuille, tous providers et tous comptes ensemble, avec un filtre. La
(b) ajoute environ **0,5 j** à L3 et change la maquette que verra Loki.

**Q3 — Le coût s'affiche-t-il en argent ?**
Assume-t-on l'affichage d'un **équivalent API en dollars**, avec la table de tarifs à maintenir
qui va avec — ou reste-t-on en tokens, quitte à perdre la seule grandeur vraiment comparable
entre modèles ? Sans « oui » ici, L2 perd l'essentiel de son intérêt et S1, S5, S9 se réduisent à
des volumes.

**Q4 — Quelle profondeur d'histoire veut-on garder ?**
Trois mois ? Un an ? Sans limite ? Cela règle la rétention des rollups de L1. Question jumelle :
veut-on **en parallèle** relever `cleanupPeriodDays` dans Claude Code pour garder les transcripts
bruts plus longtemps ? C'est possible, mais le prix est en disque — **environ 400 Mo par mois** au
rythme actuel, et croissant. Mon avis : rollups oui, relèvement de la purge non.

**Q5 — Codex reçoit-il le même traitement ?**
La déduplication n'est pas un sujet côté Codex (les `token_count` sont cumulatifs, déjà traités
par `fold_rollout`), mais l'axe modèle et le coût, si. L'aligner représenterait de l'ordre de
**+1 j** réparti sur L2 et L3. Priorité basse ou haute ?

**Q6 — Que fait-on des seaux de projet parasites ?**
Les racines de portefeuille produisent des « projets » nommés `work` ou `Desktop` (§ 4.8). Trois
options : (a) les laisser, c'est du bruit assumé ; (b) les regrouper sous « hors projet », coût
quasi nul ; (c) une table d'alias configurable, plus propre mais de la configuration à maintenir.
Je recommande **(b)**.

**Q7 — L'axe persona (S7 / L5) a-t-il de la valeur pour vous ?**
C'est 1 j, c'est le lot le plus fragile techniquement (format non documenté), et c'est le plus
spécifique à iakaframe. « Quel agent de mon équipe me coûte le plus » est une question que seul le
décideur peut dire utile ou anecdotique.

---

## 11. Sources

**Code et documents du projet** (lus pour cette proposition) :

- `specs/instructions/feature-app-analytics.md` — cadrage d'origine de la fenêtre (D1 à D5).
- `iakatc-core/src/measure/claude.rs` — `project_of:40`, `fold_line:49`, `fold_activity_line:178`,
  `fold_measure_line:275`, `scan_projects_dir:108`, `scan_projects_activity:231`,
  `scan_claude_measurements:340`.
- `iakatc-core/src/measure/mod.rs:63` — définition de `used_tokens`.
- `iakatc-core/src/aggregate.rs:14` — agrégation sans déduplication.
- `iakatc-core/src/publish/contract.rs` — codes scalaires `{v,t}`, suffixe `current`.
- `iakatc-core/src/quota/merge.rs` — réservoirs, `confidence`, `source`.
- `src-tauri/src/history.rs:107`, `src-tauri/src/state.rs:260`, `src-tauri/src/memory.rs:107`,
  `:119`, `:136`, `src/history.ts`, `src/analytics.ts`.

**Mesures sur les données réelles**, en trois temps :

1. **Campagne Aragorn** — 584 fichiers, 700 Mo, 45 jours : totaux bruts et répartition par modèle.
2. **Inspection ciblée Gandalf** — découverte des transcripts `subagents/` et du mécanisme de
   doublonnage par `message.id`, absence d'identifiant de compte, jointure `Task` → `agentId`.
   *Limite de méthode assumée : sondage sur fichiers isolés, sans Bash — d'où une extrapolation
   erronée du facteur de doublonnage.*
3. **Contre-mesure Aragorn en exécution** — corpus entier : 82 646 occurrences pour 42 865
   `message.id` (facteur 1,93), recoupement exact du `used_tokens` du daemon hors `subagents/`
   (4 987 013 378), totaux dédoublonnés par assiette (2 800 536 564 / 6 395 303 935), recouvrement
   nul entre assiettes. **C'est cette campagne qui fait foi** pour tous les chiffres de ce
   document.

**Vérifications web** (faits externes, septembre 2026) :

- Rétention des transcripts et purge par défaut à 30 jours :
  [Claude Code — Data usage](https://code.claude.com/docs/en/data-usage),
  [anthropics/claude-code #62476](https://github.com/anthropics/claude-code/issues/62476),
  [Cleanup & Retention](https://brewpirate.github.io/claude-code-docs/sessions/cleanup-retention/).
- Déduplication des enregistrements JSONL par `message.id` / `requestId` (pratique de
  l'écosystème) : [ccusage #888](https://github.com/ryoppippi/ccusage/issues/888),
  [ccusage PR #1661 — dédup scopée par session](https://github.com/ccusage/ccusage/pull/1661),
  [claude-devtools #74 — surcoût par doublons de streaming](https://github.com/matt1398/claude-devtools/issues/74),
  [anthropics/claude-code #5034 — entrées dupliquées](https://github.com/anthropics/claude-code/issues/5034).
- Tarifs par modèle et coûts relatifs du cache (entrée / sortie / lecture / création) :
  référentiel `claude-api` et [tarification Anthropic](https://www.anthropic.com/pricing).
