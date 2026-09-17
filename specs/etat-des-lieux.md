# Etat des lieux - iakaTokenCounter

> Genere par iakaframe (CLI) le 2026-09-17 14:22 (motif: manual).
> A regenerer a chaque changement de version et a chaque pause/reprise.

## Etat courant

| Champ | Valeur |
|---|---|
| Version | v0.1.0 |
| Branche | main |
| Dernier commit | 2c5005f feat(measure): fold quotidien (jour, projet, agent) pour Travail et Volume total |
| Arbre | MODIFICATIONS NON COMMITEES |
| Fichiers (suivis + non ignores) | 154 |
| Note | Re-rendu HTML apres mise a jour du recit (pause) |

## Commits recents

| Hash | Date | Sujet |
|---|---|---|
| `2c5005f` | 2026-09-17 | feat(measure): fold quotidien (jour, projet, agent) pour Travail et Volume total |
| `6efa2ab` | 2026-09-17 | fix(measure): verite des chiffres — perimetre recursif + dedup + memo (lot L0) |
| `426f57b` | 2026-09-17 | docs(instructions): ferme le jalon analytics en trois lots (L0, L1, L2) |
| `c78cc86` | 2026-09-17 | docs(proposition): outil de statistiques analytics — 7 lots, soumis a arbitrage |
| `98865c1` | 2026-09-17 | chore(iakatokencounter): update etat des lieux + commit global (pause) |
| `77e85db` | 2026-09-17 | docs(iakahub): commente le piege des deux cles retained/inflight du TOML |
| `e9e289a` | 2026-09-17 | docs(contrat-mqtt): corrige la servabilite immediate du retained + plafond de rattrapage |
| `39d4955` | 2026-09-17 | test(iakahub): caracterise le plafond de rattrapage retained contre le vrai broker |
| `a4b071d` | 2026-09-17 | feat(daemon): avertit au tick quand un filtre approche le plafond retained |
| `174fe26` | 2026-09-17 | refactor(tray): mqtt_sub consomme consumer_filters du contrat (source unique) |

## Reprise du travail (a completer par Cowork)

- **Ce qui vient d'etre fait** : correction du **transport MQTT du daemon** (lot A de
  `specs/instructions/fix-mqtt-transport-pertes.md`). Symptome signale par le decideur : la
  jauge **5h** ne s'affichait plus dans l'icone du tray. Diagnostic : le daemon poussait
  **227 codes par tick** d'un bloc dans un channel rumqttc de capacite **64** via un
  `try_publish` non bloquant dont l'`Err` etait jete sans log (`iakatc-daemon/src/mqtt.rs`).
  Mesure au faux broker : **227 emis / 65 recus**, les 65 tous de la famille
  `all/projets/agents/...` coupee alphabetiquement — **aucun code de quota ne sortait**.
  Les valeurs de quota n'arrivaient que par le resync `ConnAck`, qui iterait sur un `HashMap`
  (ordre aleatoire), d'ou un etat retained en patchwork : `remaining_pct` frais a cote d'un
  `confidence: "none"` fige depuis des heures. Or `classify()` (`src-tauri/src/icon.rs`) rend
  `Fill::Unknown` sur `confidence: "none"` → barre dessinee vide malgre `remaining_pct = 99`.
  Livre par Gimli en 5 commits atomiques (pousses sur `origin`) : capacite 1024 + inflight
  aligne, `publish_batch` a retry borne (tentatives **et** budget par lot), distinction
  `TryRequest` / `Request`, etat par topic valeur+envoye, resync delegue a un thread court
  avec ordre deterministe, log de tick honnete (`emis / publies / perdus`), cible `[lib]` +
  tests d'integration A1/A2. **Point 9** inclus sur feu vert du decideur, en commit separe :
  `DEFAULT_HOST` du daemon `192.168.2.11` → `127.0.0.1`, et § 6 de `specs/contrat-mqtt-conso.md`
  aligne. Verification apres coup sur le broker reel : **227 emis / 227 recus**, les 35 codes
  de quota presents, `claude/max` 5h et 7j en `confidence: "official"`, **tous au meme `t`**.
  App rebuildee (sidecars d'abord — le daemon n'est pas compile par le build Tauri) et
  reinstallee dans `/Applications`. Recette **a froid** validee : purge des 227 retained →
  broker a 0 → repeuplement integral en **moins de 10 s** au tick suivant.
- **Puis, dans la meme session** : **lot A clos** et **lot B livre**.
  - **Gate Legolas sur A : PASS.** Suite qualite re-executee par lui (113 tests verts, clippy 0),
    mesure A5 rejouee independamment (681 PUBLISH captures = 3 x 227, comptes cote recepteur).
    Il a tranche les deux ecarts de Gimli : le broker de test maison est **legitime** (le plafond
    d'iakahub ne frappe qu'au rattrapage de backlog, pas en fan-out live — 300/300 mesure), mais
    la justification d'infaisabilite du test A3 etait **erronee** : il a ecrit la sonde lui-meme
    en reutilisant le motif deja present dans `mqtt_no_loss.rs`. Reserve non bloquante.
  - **Reserve A3 fermee** par Gimli (`56de18e`) : test d'integration reseau coupure/reprise,
    **15/15 executions consecutives** a ~5,05 s (constance qui suit le backoff de 5 s, signe d'un
    test deterministe). L'anti-empilement (`resyncing` sous `ConnAck` repetes) reste **non
    couvert**, dit explicitement plutot que force en test fragile.
  - **Lot B livre** (`8c1f4cb`, `552f685`, `15c3607`) : dedup sur `v` seul, etat « a publier » vs
    « confirme », resync periodique tous les 10 ticks via `force_resync` (meme point d'entree que
    le resync `ConnAck`). **Gate Legolas sur B : PASS, aucune reserve bloquante.**
    **Mesure : 227 messages par tick → 1** en regime idle (227 au 1er tick, rien n'etant connu).
    Legolas a verifie le scenario adverse du silence permanent (echec tick N, valeur qui change
    puis revient) : `sent` passe a `false` **avant** la tentative et ne revient a `true` qu'apres
    succes reel, donc la dedup est court-circuitee tant que l'envoi n'est pas confirme — aucun
    chemin vers un topic muet a vie. Il a aussi **vu tourner** le resync periodique plutot que de
    le deduire : sequence `227, 1, 1, 1, 1, 1, 1, 1, 1, 1, 228, 1, 1, 1` sur 14 ticks, le burst
    tombant exactement au 10e.
  - **Cadrage `garde-plafond-retained-broker.md` depose** (`46722bb`), **non valide, non
    implemente** — voir « En cours ».
- **Troisieme lot livre : garde-plafond retained** (`43b6dda` → `77e85db`, 6 commits).
  **Les deux decisions du decideur ont ete rendues** : instruction **validee**, commentaire dans
  `iakahub/rumqttd.toml` **autorise**. Une montee de plafond a 150 a ete envisagee puis
  **annulee** par le decideur — elle etait de toute facon inoperante (le plafond est une
  constante de compilation, pas une valeur de config). **Aucun seuil n'est configurable**, et
  c'est deliberé : ni variable d'environnement, ni option de surcharge.
  Contenu : `consumer_filters` / `RETAINED_FANOUT_CEILING = 100` / `RETAINED_BACKLOG_ALERT = 80` /
  `backlog_by_filter` dans `iakatc-core/src/publish/contract.rs` (matcher MQTT local, pas de
  dependance `rumqttc` ajoutee a `core`) ; `src-tauri/src/mqtt_sub.rs` consomme les filtres du
  contrat au lieu de les reconstruire ; `warn_on_retained_backlog` au tick dans
  `iakatc-daemon/src/main.rs` (silencieux sous le seuil) ; test de caracterisation
  `iakahub/tests/retained_backlog_ceiling.rs` ; corrections du contrat MQTT ; commentaires du TOML.
  **Gate Legolas : PASS, C1 a C10, aucune reserve bloquante.** Il a verifie lui-meme le diff du
  backbone (commentaires seuls, valeurs intactes), l'absence de porte derobee sur les seuils,
  l'identite caractere pour caractere des filtres du tray, et relance **8 fois** le test hors
  seuil (assertion stricte `== 100`, pas de `#[ignore]`, pas d'assertion molle).
  **Reserve non bloquante consignee** : le garde-fou compte sur le lot complet du contrat (bon
  choix — compter sur ce qui sort apres dedup le rendrait aveugle quand rien ne bouge), mais un
  compte **retire** de la configuration laisserait son retained au broker sans disparaitre du
  compte, d'ou une sous-estimation. Aucun mecanisme de purge de compte retire n'existe, ni avant
  ni apres ce lot. A cadrer separement si le besoin apparait.
- **Quatrieme chantier : le jalon ANALYTICS** (demande du decideur : « sur double clic, une fenetre
  de statistiques de l'utilisation des IA ; propose-moi un outil de stats tres riche »).
  - **Le cadrage a d'abord decouvert deux defauts de mesure**, etablis en execution :
    1. **Les tours de sous-agents n'etaient jamais lus.** Claude Code les ecrit dans
       `<session-uuid>/subagents/` ; les trois scans de `measure/claude.rs` descendaient de deux
       niveaux et sautaient ce repertoire. **533 fichiers, 578 Mo, 69,5 % de la consommation.**
    2. **Chaque appel API etait compte 1,93 fois** en moyenne (82 646 occurrences pour 42 865
       `message.id` distincts) — une ligne par bloc de contenu, chacune reportant le meme `usage`.
    Les deux jouent en sens contraire sans se compenser : l'outil affichait **54 %** de la realite
    (facteur x1,85). **Recouvrement nul mesure** entre les deux assiettes.
  - **Cela a clos l'enigme du `used_tokens`** : hors `subagents/` et avec les doublons — exactement
    ce que faisait le daemon — on retrouve **4 987 013 378**, soit les ~4,96 Md publies sur MQTT.
  - **Proposition `proposition-analytics-riche.md`** (773 lignes, statut ARBITREE) : 7 lots, dont
    L3 a L7 **en reserve, non engages**. **Arbitrages du decideur** : jalon **L0+L1+L2**, vue en
    **tableau de bord de portefeuille** (tous comptes comparables, `open_analytics` devient une
    mise en evidence et non plus un filtre), **cout affiche en dollars equivalent API**.
  - **L0 « verite des chiffres » LIVRE, gate Legolas PASS, pousse** (`6efa2ab`) : marche recursive
    unique, dedup par fichier **au-dessus** des folds par ligne (verifies inchanges octet pour
    octet), grandeurs nommees « Travail » / « Volume total », seau « hors projet »
    (`PORTFOLIO_ROOTS`), et **memo par fichier invalide sur `(mtime, taille)`**.
    Cibles atteintes : total **9,23 Md**, split **30,4 / 69,6**, contre-epreuve a **5,00 Md**.
    **Le memo etait obligatoire** : L0 multiplie le volume scanne par 5,7 (122 Mo → 701 Mo) et le
    daemon relit tout **a chaque tick**, ce qui aurait fait ~40 Go d'I/O par heure. Mesure du
    rapport froid/chaud : **260x** (Gimli) et **1470x** (Legolas) — la valeur absolue a froid
    depend du cache OS, **c'est le rapport qui est la propriete recherchee**, pas la seconde.
  - **Reserve non bloquante de Legolas, a traiter dans l'instruction et non dans le code** : le
    critere « appels uniques 42 870 ± 5 » est **irrealiste sur des logs vivants**. Gimli mesure
    43 025, Legolas 42 990 — et celle de Legolas est **plus basse alors qu'elle est plus tardive**,
    donc ce compteur ne derive pas simplement avec le temps. Le total, le split et la contre-epreuve
    sont eux parfaitement coherents. Deux autres reserves : la contre-epreuve sur donnees reelles
    n'est pas portee par un test committe (seule la fixture l'est), et l'invalidation du memo par
    `(mtime, taille)` reste theoriquement contournable hors append-only.
- **En cours / a reprendre** : **L1 « memoire historique » etait EN COURS chez Gimli au moment de
  la pause.** Verifier `git log` et `git status` a la reprise : il a pu committer apres ce
  checkpoint. Contenu attendu : historique de quota a **90 jours** + **rollups quotidiens sans
  limite**, decalques du patron de `src-tauri/src/memory.rs`, recalcul idempotent, **aucune
  interface**. Trois vigilances transmises : ne pas annuler le gain du memo de L0 (le tick est
  retombe a ~12 ms a chaud, ne pas y ajouter une ecriture lourde), prouver l'idempotence du
  recalcul, et verifier ce que « sans limite » donne apres un an simule.
  **L1 est prioritaire en sequence bien qu'invisible** : c'est le seul lot dont la valeur depend du
  temps ecoule depuis son allumage. Le quota ne vit **nulle part** ailleurs — il vient de la
  statusline, n'est dans aucun log, et MQTT le perd a chaque redemarrage du broker ; pendant ce
  temps la purge a 30 jours de Claude Code ronge les transcripts par l'autre bout.
- **Ensuite** : gate Legolas sur L1, puis **L2 « cout equivalent API »**
  (`feature-cout-equivalent-api.md`, 2 j) — table de tarifs **nommee, isolee et datee**, date de
  validite **affichee a cote du montant sous peine d'echec du lot**, aucun multiplicateur global de
  cache (Fable 5.1 ne facture pas sa lecture au meme ratio), aucun modele rabattu sur un voisin,
  Codex present en volumes **mais sans montant**.
- **Deploiement local : fait.** L'app de `/Applications` porte **le lot A + le lot B**
  (daemon `c28f026d…`). Sidecars regeneres, bundle rebuild, remplacement et relance verifies.
  Observation en conditions reelles apres 3 ticks : quota `claude/max` en `confidence: "official"`
  sur les deux fenetres, valeurs coherentes (5h : `used_pct` 1 + `remaining_pct` 99 = 100 ;
  7j : 53 + 47 = 100), et **39 topics sur 47 figes au `t` du premier tick** — c'est-a-dire
  non republies parce qu'inchanges. La dedup travaille.
- **Prochaine etape concrete a la reprise** :
  1. `git log --oneline -5` et `git status` — **Gimli travaillait sur L1 pendant ce checkpoint**,
     son commit peut etre arrive apres. Ne rien ecraser sans regarder.
  2. Si L1 est commite : gate **Legolas**, puis push, puis **L2**.
  3. Si L1 est incomplet : relire `feature-memoire-historique.md` et relancer Gimli dessus.
- **Deploiement local** : l'app de `/Applications` porte les lots A, B et garde-plafond, **pas
  L0**. Un rebuild sera necessaire pour que la correction des chiffres soit visible dans la fenetre
  analytics. Sequence obligatoire : `bash scripts/prepare-sidecar.sh` **puis**
  `npm run tauri build` **puis** remplacement de `/Applications/iakaTokenCounter.app`.
  Sauvegardes des versions precedentes conservees dans le scratchpad de la session du 2026-09-17.
- **Candidat en reserve, jamais cadre** : la fraicheur de l'icone du tray (avant-dernier piege de
  cette liste) — meme defaut de fond que le bug qui a ouvert cette session, afficher une donnee
  avec plus de confiance qu'elle n'en merite.
- **Pieges connus** :
  - **⚠ LE CRITERE DE DIAGNOSTIC S'EST INVERSE AVEC LE LOT B — a lire avant de rediagnostiquer
    quoi que ce soit sur ce broker.** Pendant tout le lot A, le signe de sante etait « **un seul
    `t` distinct par famille** » : le daemon republiant les 227 codes a chaque tick, des `t`
    disperses trahissaient des pertes. **Depuis le lot B, cette metrique est FAUSSE** : un code
    inchange n'est plus republie, donc il **garde legitimement un `t` ancien**. Mesure reelle
    apres deploiement : 39 topics sur 47 figes au `t` du premier tick, 8 au tick suivant (ceux
    dont la valeur bougeait), `meta/daemon/last_tick_at` au tick courant (son `v` **est** l'epoch
    du tick, il change donc par construction). **C'est le fonctionnement nominal, pas une
    regression.**
    **Le bon critere est desormais la coherence des VALEURS entre elles, pas l'identite des
    timestamps** : `used_pct + remaining_pct = 100`, et `confidence != "none"` des lors que
    `remaining_pct` est non-null. Le bug d'origine se signalait par une incoherence de valeurs
    (`remaining_pct = 99` a cote de `confidence: "none"` et `used_pct: null`), pas seulement par
    des `t` divergents. Exemple sain releve apres deploiement du lot B, ou les 7 codes d'une meme
    fenetre portent deux `t` differents **et tout va bien** : `confidence`/`remaining_pct`/
    `resets_at`/`source`/`used_pct` au `t` du 1er tick (stables), `captured_at`/`used_tokens` au
    tick suivant (ils bougent).
  - **Le daemon n'est pas compile par `npm run tauri build`.** C'est un sidecar
    (`bundle.externalBin`). Toute correction dans `iakatc-daemon` exige
    `bash scripts/prepare-sidecar.sh` **avant** le build, sinon on rebundle l'ancien binaire
    dans une app neuve et rien ne change a l'ecran.
  - **Second goulot, cote broker → abonne — cadre, non corrige, et pire que suppose.** Le
    depassement de 100 retained pour un abonne qui se (re)connecte n'est **pas** une file qui
    s'ecoule : `forward_device_data` de rumqttd fait `retained_publishes.truncate(...)` puis pose
    `forward_retained = false` — **troncature definitive**, et le sous-ensemble conserve est un
    **tirage arbitraire** (iteration de `HashMap`). Sur un quota, cela reproduit a l'identique le
    patchwork du lot A : `remaining_pct` present, `confidence` absent.
    **Piege majeur : la cle `max_inflight_count` du TOML ne fait PAS ce que son nom dit.** Elle
    est passee a `Network::new(...)` en position `max_connection_buffer_len` (taille de tampon
    reseau) ; la vraie fenetre vient de `MAX_INFLIGHT`, **constante de compilation valant 100**
    dans rumqttd. **La relever serait un no-op doublé d'une fausse securite.** Monter de version
    ne resout rien : le `main` amont porte la meme troncature et la meme constante.
    **Le tray n'est pas expose aujourd'hui** : ses deux filtres ne couvrent que **39** topics
    (35 quota + 4 meta), pas 227 — le `+/+` du filtre quota ne franchit pas le segment `agents`
    et le segment litteral `quota` exclut `conso` et `limits`. Et le **demarrage de l'app n'est
    pas le cas expose** (le broker renait vide avec elle, le tray est deja souscrit quand le
    daemon repeuple : c'est du fan-out live, mesure a 300/300). Le scenario reellement expose est
    la **reconnexion MQTT du tray sur un broker reste vivant**.
    (Le meme plafond, plus `max_outgoing_packet_count = 200` — qui n'agit que sur les abonnes
    QoS 0, donc les sondes de debug — fausse tout sniffer MQTT maison : des totaux pile a 100 ou
    200 sont des artefacts de transport, pas des inventaires. Ce piege a reellement egare le
    diagnostic de cette session.)
  - **Depuis le lot B, le resync periodique est le SEUL chemin de reparation** d'un abonne dont
    les retained ont ete tronques. Avant B, le daemon republiait tout a chaque tick et un abonne
    tronque se reparait en ≤ 60 s. Apres B, le rattrapage est borne par la periode du resync,
    soit **≤ 10 min**. **Ne pas espacer `PERIODIC_FULL_RESYNC_EVERY_N_TICKS` sans arbitrage du
    decideur** — c'est documente dans le code (`mqtt.rs`, `main.rs`).
  - **iakahub ne persiste pas les retained sur disque.** Tout redemarrage de l'app repart d'un
    broker vide. C'est ce qui explique les timestamps de 13 jours observes avant l'intervention :
    iakahub tournait sans interruption depuis le 2 septembre et les topics orphelins
    s'accumulaient faute de redemarrage.
  - **L'etat du tray est purement additif, et l'icone ne sait pas vieillir** (verifie en lecture,
    pas suppose). Un payload vide fait echouer `parse_payload` (`src-tauri/src/state.rs`) et le
    message est ignore — d'ou la robustesse a une purge, mais **aucun code deja appris n'est
    jamais oublie**. La webview a trois seuils de fraicheur locaux (`src/render.ts:19-22` :
    5h = 1200 s, 7j = 21600 s, 30j = 86400 s) et bascule le badge en « perime ⟳ » au-dela.
    **`icon.rs` n'a aucune constante de fraicheur** : `updated_at` n'y sert qu'a `has_window()`
    (presence), jamais a juger une peremption ; son seul `Fill::Stale` vient de
    `confidence == "official_stale"`, verdict emis par le **daemon**, pas une mesure locale.
    Et `daemon_available` ne rattrape rien : `mqtt_sub.rs` ne fait qu'un `swap(true)` a la
    premiere meta recue — **aucun chemin ne le remet a `false`**, il detecte une naissance,
    jamais une mort, et le rendu de l'icone ne le consulte pas.
    **Consequence** : daemon mort ⇒ l'icone affiche une barre pleine et nette indefiniment sur
    une donnee perimee, pendant que le popover, lui, afficherait « perime ». Meme defaut de fond
    que le bug de cette session — afficher une donnee avec plus de confiance qu'elle n'en merite.
    **Candidat a cadrer**, non traite.
  - **Remotes non conformes a la methode** : `origin` → `192.168.1.139:3001`, `iakabox` →
    `192.168.2.11:3001` (deux Forgejo LAN a des adresses differentes), plus un remote `github`
    vers `github.com/iakasju/iakaTokenCounter.git`. **Aucun remote VPS `git.naonedge.com`**,
    pourtant remote par defaut de la methode. Seul `origin` a ete pousse.

## Journal (versions & pauses)

| Date | Motif | Version | Branche | Note |
|---|---|---|---|---|
| 2026-09-17 14:22 | manual | v0.1.0 | main | Re-rendu HTML apres mise a jour du recit (pause) |
| 2026-09-17 14:21 | pause | v0.1.0 | main | Jalon analytics en cours : L0 (verite des chiffres) livre, gate Legolas PASS et pousse. L1 (memoire historique) en cours chez Gimli au moment de la pause. |
| 2026-09-17 09:09 | pause | v0.1.0 | main | Troisieme lot livre : garde-plafond retained, gate Legolas PASS (C1-C10). Les deux decisions du decideur sont rendues, aucune en attente. |
| 2026-09-17 08:36 | manual | v0.1.0 | main | Lot B deploye sur le poste : app rebuildee et reinstallee, dedup verifiee en conditions reelles |
| 2026-09-17 01:52 | manual | v0.1.0 | main | Re-rendu HTML apres mise a jour du recit de reprise |
| 2026-09-17 01:50 | pause | v0.1.0 | main | Lot A clos (reserve A3 fermee) + lot B livre et gate Legolas PASS : 227 messages par tick -> 1 en regime stable. Cadrage garde-plafond retained depose, en attente d'arbitrage. |
| 2026-09-17 00:54 | pause | v0.1.0 | main | Correctif transport MQTT sans perte (lot A + point 9) : 227 emis / 227 recus au lieu de 65, quota de nouveau publie, app rebuildee et reinstallee, recette a froid validee |
| 2026-09-17 00:53 | pause | v0.1.0 | main | Correctif transport MQTT sans perte (lot A + point 9) : 227 emis / 227 recus au lieu de 65, quota de nouveau publie, app rebuildee et reinstallee, recette a froid validee |
