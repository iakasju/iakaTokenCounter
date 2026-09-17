# Instruction : Vérité des chiffres (périmètre de scan + déduplication + grandeur nommée)

> Rédigé par Gandalf (P1 — cadrage). Consommé par Gimli (Claude Code) comme instruction de
> travail. Doc en français, code/identifiants en anglais.
> **Lot L0** du jalon arbitré par le décideur le 2026-09-17 (L0 + L1 + L2, **6 j** — L0 révisé de
> 1,5 à **2 j** par la décision D5 ci-dessous).
> Proposition d'origine et mesures : `specs/instructions/proposition-analytics-riche.md`.
>
> **Périmètre : `iakatc-core` (lecture seule) + libellés de la webview.** Aucune modification du
> contrat MQTT, de la cadence du daemon, ni de l'architecture des fenêtres.

---

## Contexte

La fenêtre analytics et le daemon comptent aujourd'hui **54 % de la consommation réelle**. Deux
défauts indépendants, mesurés sur les données réelles du décideur (45 jours, 584 fichiers,
700 Mo) :

1. **Le scan ne lit pas les transcripts de sous-agents.** Claude Code écrit les tours délégués
   dans `<session-uuid>/subagents/agent-<id>.jsonl` — un niveau **plus bas** que ce que parcourent
   les trois fonctions de scan. 533 fichiers, 129 283 tours, **6 395 303 935 tokens** : jamais lus.
2. **Chaque appel API est compté 1,93 fois en moyenne.** Un même message d'assistant est écrit sur
   une ligne JSONL par bloc de contenu (texte, puis chaque `tool_use`), et **chaque ligne reporte
   le même bloc `usage` à l'identique**. 82 646 occurrences pour 42 865 appels réels.

S'y ajoute un troisième défaut, de nature différente : **le code porte trois définitions
incompatibles de « tokens consommés »**, dont deux s'affichent côte à côte dans la même fenêtre
sans se nommer, avec un écart de facteur ≈ 25 entre elles.

Ce lot **n'ajoute aucune visualisation**. Il rend vraies les quatre qui existent déjà.

## ⚠️ Les deux corrections ne sont pas séparables — à lire avant de découper

**Ne livre pas P1 sans P2, ni P2 sans P1.** Les deux défauts jouent en sens contraire :

| Si tu livres… | Effet sur les chiffres affichés | Ce que le décideur en conclura |
|---|---|---|
| **P2 seule** (déduplication) | **chute de moitié** (4,99 Md → 2,80 Md) | « on a cassé la mesure » — alarme injustifiée |
| **P1 seule** (périmètre) | **triplement** (4,99 Md → 16,87 Md) | « la conso a explosé » — euphorie injustifiée |
| **P1 + P2 ensemble** | 4,96 Md → **9 195 840 499** | la vérité, ×1,85 |

Les deux corrections peuvent être développées et testées séparément — **elles doivent atterrir
dans le même commit livrable**, ou à tout le moins ne jamais être exposées à l'utilisateur l'une
sans l'autre. Aucun `message.id` n'est partagé entre les deux assiettes (recouvrement mesuré
**nul**) : il n'y a pas de double comptage croisé à craindre, les deux corrections sont
orthogonales.

## Faits vérifiés (mesures sur les données réelles, 2026-09-17)

| Assiette | Tokens dédoublonnés | Appels API uniques |
|---|---|---|
| Hors `subagents/` (ce que le scan voit) | **2 800 536 564** | 7 510 |
| Dans `subagents/` (ce qu'il ignore) | **6 395 303 935** | 35 360 |
| **Total réel** | **9 195 840 499** | 42 870 \* |
| *Publié aujourd'hui par le daemon* | *≈ 4 960 000 000* | — |

> \* Somme des deux assiettes ; le dénombrement global donne 42 865 identifiants distincts —
> écart de 5 appels (0,01 %) sans incidence sur les volumes, à élucider si le compte exact
> importe (probable effet de bord aux frontières de session).

**Contre-épreuve du diagnostic** : en scannant hors `subagents/` **en gardant les doublons** —
c'est-à-dire exactement ce que fait le daemon aujourd'hui — on obtient **4 987 013 378** tokens,
contre ≈ 4,96 Md publiés. C'est la même valeur : le périmètre de scan explique l'écart en
totalité.

**Forme réelle de l'arborescence** :

```
~/.claude/projects/<cwd-échappé>/<session-uuid>.jsonl                        ← lu
~/.claude/projects/<cwd-échappé>/<session-uuid>/subagents/agent-<id>.jsonl   ← JAMAIS lu
```

Les transcripts de sous-agents portent **`cwd`**, **`timestamp`**, **`message.usage`**,
**`message.model`** et **`isSidechain:true`** — même forme que les transcripts principaux. Ils
sont donc exploitables par les folds existants **sans adaptation du parsing**.

**Zéro tour `isSidechain:true` dans les transcripts principaux** (`*/*.jsonl`) : le sidechain
en ligne a disparu, tout est passé dans `subagents/`.

**Qualité de la source** : 82 624 lignes `usage`, **0 ligne illisible**. Le parsing défensif
existant n'a rien à corriger de ce côté.

---

## Ce qui existe (à réutiliser)

| Élément | Où | Rôle pour ce lot |
|---|---|---|
| Scan économie (treemap) | `iakatc-core/src/measure/claude.rs::scan_projects_dir:108` | À rendre récursif + dédupliqué |
| Scan activité (timeline) | `claude.rs::scan_projects_activity:231` | Idem |
| Scan mesure (daemon/MQTT) | `claude.rs::scan_claude_measurements:340` | Idem |
| Folds par ligne, purs et testés | `claude.rs::fold_line:49`, `fold_activity_line:178`, `fold_measure_line:275` | **À NE PAS toucher** — la déduplication se fait au-dessus |
| **Patron de marche récursive déjà écrit** | `iakatc-core/src/measure/codex.rs::walk_jsonl:57` | **Le modèle exact** : pile de répertoires, best-effort, jamais d'erreur propagée |
| Extraction de clé projet | `claude.rs::project_of:40` | À documenter, pas à réécrire |
| Fixtures Claude | `specs/mock/claude_projects/` | À étendre (sous-agents + doublons) |
| Libellés de la webview | `src/history.ts` (`historyTimeline`, `historyTreemap`, `historySplit`) | Y porter le nom de la grandeur |

## Décision

### D1 — Un seul point d'entrée pour la liste des transcripts, récursif

**Retenu** : extraire une fonction **`claude_transcript_files(projects_dir) -> Vec<PathBuf>`** qui
descend **récursivement** sous `projects_dir` et renvoie tous les `*.jsonl`, et faire passer les
**trois** scans par elle.

**Pourquoi** : les trois scans dupliquent aujourd'hui la même double boucle `read_dir` (trois
occurrences de la même logique, trois occasions de diverger). Un point d'entrée unique garantit
que **les trois voient la même chose** — c'est précisément ce qui manque aujourd'hui entre le
daemon et la vue. Le patron est déjà écrit et éprouvé dans le projet (`codex.rs::walk_jsonl`) :
pile de répertoires, `flatten()` sur les entrées, répertoire illisible **sauté sans erreur**.

**Écarté** : ajouter un niveau de boucle en dur dans chaque scan (`<sid>/subagents/*.jsonl`). Cela
marcherait aujourd'hui et casserait au prochain changement d'arborescence de l'éditeur. La marche
récursive ne fait aucune hypothèse sur la profondeur.

> **Micro-choix tranché** : récursion **sans limite de profondeur**, filtrage sur la seule
> extension `.jsonl`. Si l'éditeur déplace ou renomme `subagents/`, le scan continue de trouver
> les fichiers.

### D2 — La déduplication se fait **au niveau du fichier**, les folds par ligne restent intacts

**Retenu** : introduire, pour chaque famille de scan, une fonction **`fold_file_*(acc, content)`**
qui :

1. parcourt les lignes une première fois et construit une table **`HashMap<&str, &str>`** de
   `message.id` → **dernière ligne portant cet identifiant** (emprunts sur `content`, aucune copie) ;
2. délègue ensuite chaque ligne retenue au fold par ligne **existant, inchangé**.

Une ligne **sans** `message.id` exploitable est conservée telle quelle (défensif : on ne perd
jamais de donnée par excès de zèle).

**Pourquoi « au niveau du fichier »** : un fichier = **une session** (transcript principal, ou un
run de sous-agent). Déduplicer par fichier, c'est exactement la règle « dédup scopée par session »
que l'écosystème a convergé à appliquer — et cela évite d'écraser deux sessions distinctes qui
réutiliseraient un identifiant.

**Pourquoi « dernière occurrence »** : sur les données du décideur, les occurrences d'un même
`message.id` portent un `usage` **strictement identique** — premier ou dernier revient au même.
Mais des cas rapportés dans l'écosystème décrivent une **première occurrence partielle** (instantané
intermédiaire) et une **dernière complète**. Garder la dernière est donc **strictement plus sûr**,
à coût nul.

**Pourquoi ne pas toucher aux folds par ligne** : ils sont purs, publics et couverts par une
batterie de tests qui valident le portage depuis `economy.rs`. Changer leur signature ferait payer
au lot un coût de régression sans rapport avec son objet.

> **Micro-choix tranché** : la table emprunte des `&str` sur le contenu déjà chargé en mémoire
> (`read_to_string` charge déjà le fichier entier) — **aucune allocation supplémentaire par ligne**.

### D3 — Deux grandeurs, nommées à l'écran

Le code porte trois règles ; l'écran n'en nomme aucune. **Retenu** : conserver **deux** grandeurs,
chacune explicitement libellée dans la vue :

| Nom à l'écran | Formule | Où elle s'affiche |
|---|---|---|
| **« Travail »** | `entrée fraîche + création de cache + sortie` (**hors cache réutilisé**) | Timeline d'activité |
| **« Volume total »** | `entrée + création de cache + cache réutilisé + sortie` | Treemap, split, mesure MQTT |

**Pourquoi deux et pas une** : elles répondent à deux questions différentes du décideur — « combien
ai-je fait travailler l'IA » et « combien cela pèse ». Les fusionner ferait perdre l'une des deux.

**La règle absolue** : **elles ne se mélangent jamais dans une même visualisation**, et chaque
visualisation **affiche le nom de la grandeur qu'elle montre**. C'est la même honnêteté que le
bandeau de portée (D4 de `feature-app-analytics.md`) et que les badges `confidence` du contrat.

> **Note pour L2** : « Volume total » en tokens bruts est une grandeur de transition. Dès que le
> coût par modèle existe (lot L2), **le poids s'exprime en dollars**, et le nombre de tokens passe
> en second rang. Ne pas sur-investir dans sa présentation ici.

### D4 — La définition du « projet » est posée noir sur blanc

Le nom de projet n'est **pas** le nom du répertoire de `~/.claude/projects` : il est extrait du
**contenu**, ligne par ligne, par `project_of` — **dernier segment du `cwd`** de chaque
enregistrement. D'où 44 projets publiés quand l'arborescence ne montre que 12 répertoires actifs.

**Retenu** : **conserver cette règle** — elle est correcte, et elle devient meilleure avec P1
(un sous-agent qui opère dans un autre répertoire que sa session est attribué au bon projet).
Trois précisions à inscrire :

1. **Documenter la règle en tête de module et à l'écran** (infobulle ou bandeau) : « projet =
   dernier segment du répertoire de travail ».
2. **Seau « hors projet »** : les racines de portefeuille produisent des projets nommés `work`,
   `Desktop`… Ce ne sont pas des projets. Les regrouper sous un seau unique **« hors projet »**,
   via une **constante nommée et isolée** (`PORTFOLIO_ROOTS`), pas une liste éparpillée.
3. **Collision de feuilles assumée** : `/a/web` et `/b/web` fusionnent. Non résolu ici (il faudrait
   changer la clé) — **mentionné dans l'infobulle**, qui affiche le `cwd` complet.

**Écarté** : une table d'alias configurable (configuration à maintenir, sur-ingénierie au regard
du gain) ; et la détection automatique des racines par inclusion de chemins (élégante, mais
complexité sans rapport avec l'objet du lot).

### D5 — Mémo par fichier sur `mtime` : **obligatoire, et dans ce lot**

**Le problème que crée P1, mesuré.** Ce lot multiplie l'assiette de scan par **5,7** :

| | Fichiers | Volume par scan |
|---|---|---|
| Avant L0 (hors `subagents/`) | 52 | **122,4 Mo** |
| Après L0 (tout) | 585 | **701,2 Mo** |

Ce serait anodin si le seul consommateur était la fenêtre analytics — on attend volontiers deux
secondes après un double-clic. **Mais le daemon re-scanne l'intégralité des logs à chaque tick,
toutes les 60 secondes, en permanence** (`iakatc-daemon/src/main.rs:82` → `:88`, boucle
`main.rs:76`). Livrer P1 sans rien d'autre ferait passer la lecture de fond de 122 Mo à **701 Mo
par minute, soit de l'ordre de 40 Go d'I/O par heure**, sur le poste du décideur, pour une
application de tray censée être discrète. **Ce n'est pas une question de latence ressentie, c'est
une question d'usure SSD, de CPU et de batterie.**

**C'est L0 qui crée cette charge : c'est donc à L0 de la neutraliser.** Réparer les chiffres en
dégradant le comportement serait exactement le troc que cette session refuse depuis le début.

**Retenu — un mémo par fichier, invalidé par `(mtime, taille)`** :

- avant chaque scan, parcourir l'arborescence en ne lisant que les **métadonnées** (quelques
  millisecondes pour 585 entrées, aucun contenu lu) ;
- pour chaque fichier, si `(mtime, taille)` est **inchangé** depuis le dernier scan → **réutiliser
  son résultat partiel mémorisé** ; sinon → lire, replier, mémoriser ;
- **sommer les résultats partiels** de tous les fichiers.

**Pourquoi c'est correct, et pourquoi c'est peu de code.** Les trois accumulateurs sont **purement
additifs** (`(projet, agent) → Tokens`, `projet → (input, output, coord, sub)`,
`projet → jour → tokens`) : le résultat global est la somme des résultats par fichier, quel que
soit l'ordre. Et — c'est la clé — **la déduplication de D2 est scopée au fichier** : aucun
identifiant n'est partagé entre fichiers (recouvrement mesuré nul), donc **un résultat par fichier
est calculable indépendamment et reste valide tant que le fichier ne change pas**. La mémoïsation
par fichier n'est pas un bricolage opportuniste : elle est *permise par* la décision D2.

**Ce que cela ne change pas** : le daemon continue de **recalculer depuis le disque**, sans
incrément mémoire approximatif — la propriété que revendique le commentaire de `tick()` est
préservée. Les totaux restent **exacts**, pas estimés. Un redémarrage repart d'un mémo vide : le
premier tick fait un scan complet (0,3 à 0,7 s en Rust), les suivants sont quasi gratuits, puisque
**l'écrasante majorité des 585 fichiers est inerte d'un tick à l'autre** — seule la session en
cours grossit.

**Périmètre du mémo dans ce lot** : **le daemon uniquement**, donc le seul
`scan_claude_measurements`. C'est lui qui tourne en continu.

**Écarté** :

- **Un index incrémental persistant sur disque** (ce que la proposition chiffrait en lot L7 à 1 j) :
  plus cher, à invalider, à migrer, à réparer quand il se corrompt. Le mémo en mémoire suffit,
  parce que le coût qu'on veut supprimer est **répété**, pas initial.
- **Ignorer les fichiers anciens** (par exemple « ne relire que les 7 derniers jours ») : casserait
  les totaux cumulés que publie le contrat.
- **Espacer les ticks du daemon** : changerait le contrat de fraîcheur pour masquer un problème de
  volume. On traite la cause.

> **Conséquence sur le lot L7 de la proposition** : sa part **nécessaire** est absorbée ici. Le
> reliquat — mémo côté GUI, index persistant — **reste en réserve, non engagé**, et n'est justifié
> par aucune mesure : 0,3 à 0,7 s à l'ouverture d'une fenêtre ouverte à la demande est acceptable.

> **Conséquence sur l'estimation** : L0 passe de **1,5 j à 2 j**. Le jalon engagé passe à **6 j**
> (L0 2 j + L1 2 j + L2 2 j).

---

## Étapes d'implémentation

1. **Fixtures d'abord.** Étendre `specs/mock/claude_projects/` avec (a) un répertoire
   `<session-uuid>/subagents/agent-<id>.jsonl` portant des tours `isSidechain:true` datés et
   attribués à un projet, et (b) **un même `message.id` répété sur 3 lignes** avec un `usage`
   identique. Les tests de non-régression existants doivent continuer de passer **avant** toute
   modification de production.
2. **`claude_transcript_files(projects_dir) -> Vec<PathBuf>`** (D1) : marche récursive calquée sur
   `codex.rs::walk_jsonl:57`, filtrage `.jsonl`, défensive (répertoire illisible sauté). Test :
   sur la fixture, elle trouve **le transcript principal ET le fichier de sous-agent**.
3. **Brancher les trois scans dessus** (`scan_projects_dir`, `scan_projects_activity`,
   `scan_claude_measurements`) en supprimant les trois doubles boucles `read_dir`.
4. **`fold_file_*` + déduplication** (D2) : pour chacune des trois familles, une fonction de
   niveau fichier qui construit la table `message.id → dernière ligne` puis délègue aux folds par
   ligne **inchangés**. Test : la fixture à `message.id` triplé produit **le tiers** du total naïf.
5. **Mémo par fichier** (D5) : structure `ScanCache` dans `iatc-core` (clé = chemin, valeur =
   `(mtime, len, résultat partiel)`), variante `scan_claude_measurements_cached(dir, &mut cache)`.
   La fonction non mémoïsée **reste disponible** pour la GUI et les tests.
6. **Câbler le daemon** : `tick()` (`main.rs:82`) détient le `ScanCache` d'un tick à l'autre et
   appelle la variante mémoïsée. Aucun autre changement du daemon.
7. **Grandeur nommée** (D3) : porter le libellé « Travail » / « Volume total » dans les titres des
   trois visualisations (`src/history.ts`), avec une mention courte de la formule en infobulle.
8. **Définition du projet** (D4) : documenter la règle en tête de `claude.rs`, introduire la
   constante `PORTFOLIO_ROOTS` et le seau « hors projet », afficher le `cwd` complet en infobulle.
9. **Bandeau de révision des chiffres** : avertir **dans la vue** que les valeurs ont changé de
   définition et d'assiette avec cette version (une ligne, pas une modale) — sans quoi le décideur
   lira une régression là où il y a une correction.
10. **Mesurer et consigner** : temps et volume d'un tick avant / après, avec et sans mémo. Chiffres
    reportés dans le message de remise.

## Fichiers concernés

- `iakatc-core/src/measure/claude.rs` — `claude_transcript_files` (nouveau), trois scans rebranchés,
  trois `fold_file_*` (nouveaux), `PORTFOLIO_ROOTS` + seau « hors projet », doc de la règle projet.
- `iakatc-core/src/measure/mod.rs` — `ScanCache` (ou module dédié `measure/cache.rs`).
- `iakatc-daemon/src/main.rs` — `tick()` détient et passe le `ScanCache`.
- `src/history.ts` — libellés de grandeur, infobulles (`cwd` complet), bandeau de révision.
- `src/analytics.ts` — bandeau de révision.
- `specs/mock/claude_projects/` — fixtures sous-agents + doublons.
- Tests : `#[cfg(test)]` dans `claude.rs` (marche récursive, déduplication, mémo), tests front des
  libellés.

## Comportement attendu

**Critères d'acceptation chiffrés** — ce lot a la rare propriété de connaître ses résultats
attendus avant d'être écrit. Sur les logs réels du décideur (45 jours, 2026-08-03 → 2026-09-17) :

- [ ] Le total Claude *all-time* passe de `≈ 4,96 Md` à **9 195 840 499** tokens.
- [ ] Il se décompose en **2 800 536 564** (hors `subagents/`) + **6 395 303 935** (dans
      `subagents/`).
- [ ] Le split coordinateur / sous-agent affiche **≈ 30 / 70** (et non 100 / 0).
- [ ] Le nombre d'appels API uniques retenus est de **42 870 ± 5**.
- [ ] **Contre-épreuve du diagnostic** : en désactivant la récursion *et* la déduplication, on
      retrouve **4 987 013 378** — la valeur publiée aujourd'hui. Si ce nombre n'apparaît pas, le
      modèle du défaut est faux et il faut s'arrêter pour comprendre.

> Ces valeurs sont datées. Elles dérivent si les logs changent (nouvelle activité, purge à 30 j).
> **Les vérifier sur un instantané figé des logs**, pas sur le dossier vivant — ou accepter une
> dérive et vérifier les **rapports** (×1,85 ; 30 / 70) plutôt que les valeurs absolues.

**Critères fonctionnels** :

- [ ] Chaque visualisation **affiche le nom de la grandeur** qu'elle montre (« Travail » ou
      « Volume total »).
- [ ] Les projets issus d'une racine de portefeuille apparaissent sous **« hors projet »**, et
      l'infobulle d'un projet montre le **`cwd` complet**.
- [ ] Un répertoire de logs absent ou illisible donne des **séries vides**, jamais une erreur
      (propriété défensive existante préservée).
- [ ] **Coût permanent du daemon** : après un premier tick complet, les ticks suivants ne relisent
      que les fichiers effectivement modifiés. Vérifiable en observant que le volume lu par tick
      retombe de 701 Mo à quelques Mo.
- [ ] Les totaux publiés par le daemon avec mémo sont **identiques** à ceux calculés sans mémo
      (test : deux scans successifs, l'un mémoïsé, l'autre non, sur la même fixture).

## Vérification

- [ ] `cargo check` / typecheck front OK
- [ ] `cargo clippy` + lint front OK
- [ ] `cargo test` vert, dont : marche récursive (trouve les sous-agents), déduplication (facteur
      exact sur fixture), équivalence mémo / sans mémo, invalidation du mémo quand un fichier
      grossit
- [ ] `bash scripts/quality-report.sh` OK
- [ ] Testé dans l'app réelle : les cinq critères chiffrés ci-dessus, relevés et consignés
- [ ] Mesure avant / après du coût d'un tick du daemon, consignée dans le message de remise

## Hors scope

- **Toute nouvelle visualisation** — ce lot ne fait que rendre vraies celles qui existent.
- **L'axe modèle et le coût en dollars** → lot L2 (`feature-cout-equivalent-api.md`).
- **L'historisation et les rollups** → lot L1 (`feature-memoire-historique.md`).
- **La vue portefeuille** (tous comptes / tous providers) → lot L2.
- **Codex** : ni le doublonnage ni les sous-agents ne le concernent (`fold_rollout` retient déjà le
  maximum cumulatif, et Codex n'a pas de sidechain). Aucune modification de `codex.rs` dans ce lot.
- **Index persistant sur disque, mémo côté GUI** : reliquat du lot L7, non engagé (D5).
- **Toute modification du contrat MQTT, des topics, de la cadence du daemon.**

## Sources

- Proposition et mesures d'origine : `specs/instructions/proposition-analytics-riche.md`
  (§ 4.1 périmètre, § 4.2 doublons, § 4.3 grandeurs, § 4.8 granularité projet).
- Cadrage d'origine de la vue : `specs/instructions/feature-app-analytics.md` (D2, D4, D5).
- Contre-mesures en exécution sur les données réelles (Aragorn, 2026-09-17) : totaux dédoublonnés
  par assiette, recoupement du `used_tokens` du daemon, volumes et chronométrage du scan.
- Patron de marche récursive : `iakatc-core/src/measure/codex.rs:57`.
- Point de charge permanent : `iakatc-daemon/src/main.rs:76` (boucle), `:82` (`tick`), `:88`.
