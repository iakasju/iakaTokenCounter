# Etat des lieux - iakaTokenCounter

> Genere par iakaframe (CLI) le 2026-09-17 01:50 (motif: pause).
> A regenerer a chaque changement de version et a chaque pause/reprise.

## Etat courant

| Champ | Valeur |
|---|---|
| Version | v0.1.0 |
| Branche | main |
| Dernier commit | 46722bb docs(instructions): cadre le garde-fou du plafond de retained cote broker |
| Arbre | propre |
| Fichiers (suivis + non ignores) | 145 |
| Note | Lot A clos (reserve A3 fermee) + lot B livre et gate Legolas PASS : 227 messages par tick -> 1 en regime stable. Cadrage garde-plafond retained depose, en attente d'arbitrage. |

## Commits recents

| Hash | Date | Sujet |
|---|---|---|
| `46722bb` | 2026-09-17 | docs(instructions): cadre le garde-fou du plafond de retained cote broker |
| `15c3607` | 2026-09-17 | test(daemon): couvre B1/B3 par des tests d'integration (dedup + resync) |
| `552f685` | 2026-09-17 | docs(mqtt): le resync periodique est le seul chemin de reparation d'un abonne tronque |
| `8c1f4cb` | 2026-09-17 | feat(mqtt): publication differentielle + resync periodique (lot B) |
| `56de18e` | 2026-09-17 | test(daemon): couvre A3 (resync sur reconnexion) par un test d'integration reseau |
| `ec59f3a` | 2026-09-17 | chore(iakatokencounter): update etat des lieux + commit global (pause) |
| `9db95bf` | 2026-09-16 | fix(config): defaut de broker du daemon aligne sur iakahub local (127.0.0.1) |
| `2207aec` | 2026-09-16 | test(daemon): integration A1/A2 — lot de 300 sans perte, hors-ligne borne |
| `8879652` | 2026-09-16 | fix(mqtt): transport sans perte, budget de retry borne, resync hors event-loop |
| `350ab4a` | 2026-09-16 | chore(daemon): expose config/mqtt via une cible [lib] |

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
- **En cours / a reprendre** : rien en cours, arbre propre, `main` synchronise avec `origin`.
  **Deux decisions attendent le decideur :**
  1. **Valider ou non l'instruction `specs/instructions/garde-plafond-retained-broker.md`**
     (~0,5 j-h). Conclusion de Gandalf : *rien a reparer aujourd'hui, tout a outiller*. Marge
     chiffree : **3 comptes IA** avant de franchir le seuil (7 topics par reservoir, seuil atteint
     au 8e compte ; 5 reservoirs aujourd'hui).
  2. **Autoriser ou non une touche unique a `iakahub/rumqttd.toml`** : un commentaire disant ce
     que les deux cles font reellement, **aucune valeur modifiee**. iakahub etant un backbone
     partage, l'arbitrage appartient au decideur. Refus coherent : le commentaire irait alors
     dans le contrat MQTT seul.
- **Prochaine etape concrete** : **rebuild + reinstallation de l'app** pour que le lot B prenne
  effet sur le poste. Le binaire de `/Applications` porte aujourd'hui le lot A seul.
  Sequence obligatoire : `bash scripts/prepare-sidecar.sh` **puis** `npm run tauri build` **puis**
  remplacement de `/Applications/iakaTokenCounter.app` (voir le premier piege ci-dessous).
- **Pieges connus** :
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
| 2026-09-17 01:50 | pause | v0.1.0 | main | Lot A clos (reserve A3 fermee) + lot B livre et gate Legolas PASS : 227 messages par tick -> 1 en regime stable. Cadrage garde-plafond retained depose, en attente d'arbitrage. |
| 2026-09-17 00:54 | pause | v0.1.0 | main | Correctif transport MQTT sans perte (lot A + point 9) : 227 emis / 227 recus au lieu de 65, quota de nouveau publie, app rebuildee et reinstallee, recette a froid validee |
| 2026-09-17 00:53 | pause | v0.1.0 | main | Correctif transport MQTT sans perte (lot A + point 9) : 227 emis / 227 recus au lieu de 65, quota de nouveau publie, app rebuildee et reinstallee, recette a froid validee |
