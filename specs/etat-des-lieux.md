# Etat des lieux - iakaTokenCounter

> Genere par iakaframe (CLI) le 2026-09-17 00:54 (motif: pause).
> A regenerer a chaque changement de version et a chaque pause/reprise.

## Etat courant

| Champ | Valeur |
|---|---|
| Version | v0.1.0 |
| Branche | main |
| Dernier commit | 9db95bf fix(config): defaut de broker du daemon aligne sur iakahub local (127.0.0.1) |
| Arbre | MODIFICATIONS NON COMMITEES |
| Fichiers (suivis + non ignores) | 143 |
| Note | Correctif transport MQTT sans perte (lot A + point 9) : 227 emis / 227 recus au lieu de 65, quota de nouveau publie, app rebuildee et reinstallee, recette a froid validee |

## Commits recents

| Hash | Date | Sujet |
|---|---|---|
| `9db95bf` | 2026-09-16 | fix(config): defaut de broker du daemon aligne sur iakahub local (127.0.0.1) |
| `2207aec` | 2026-09-16 | test(daemon): integration A1/A2 — lot de 300 sans perte, hors-ligne borne |
| `8879652` | 2026-09-16 | fix(mqtt): transport sans perte, budget de retry borne, resync hors event-loop |
| `350ab4a` | 2026-09-16 | chore(daemon): expose config/mqtt via une cible [lib] |
| `787ba91` | 2026-09-16 | docs(instructions): cadre le correctif du transport MQTT sans perte |
| `d209314` | 2026-08-05 | docs(readme): liste les binaires reellement publies, tous systemes |
| `97843c9` | 2026-08-05 | docs(readme): le contrat de projet est celui du runner, pas d'un produit |
| `a36347c` | 2026-08-05 | ci(release): construit les sidecars pour la cible avant le bundle |
| `f8b7bf9` | 2026-08-05 | ci(release): choix des plateformes au declenchement manuel |
| `0e66227` | 2026-08-05 | docs(readme): l'installation part du binaire publie, plus des sources |

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
- **En cours / a reprendre** : rien en cours, arbre propre, `main` synchronise avec `origin`.
  Le **lot B** (publication differentielle / dedup) est **cadre et non demarre** : il est
  specifie dans la meme instruction, avec deux points de conception deja tranches par Gandalf
  (dedup sur `v` seul, car `t` change a chaque tick ; etat memoire distinguant *derniere valeur
  a publier* de *envoi confirme*, sinon un message perdu ne serait plus jamais reemis).
- **Prochaine etape concrete** : faire passer **Legolas** sur le lot A. Gimli a fait tourner
  `scripts/quality-report.sh` en PASS lui-meme, mais le gate independant n'a pas eu lieu, et il
  a laisse **deux ecarts assumes a trancher** : (a) le broker de test reimplemente dans
  `iakatc-daemon/tests/mqtt_no_loss.rs` au lieu de reutiliser `iakahub::broker`, parce que le
  `max_inflight_count = 100` d'iakahub fait cesser la redistribution au-dela du seuil ;
  (b) le critere A3 (resync apres coupure reelle) couvert par un test unitaire d'ordonnancement
  plutot qu'en integration, faute d'API de redemarrage propre cote `rumqttd`.
- **Pieges connus** :
  - **Le daemon n'est pas compile par `npm run tauri build`.** C'est un sidecar
    (`bundle.externalBin`). Toute correction dans `iakatc-daemon` exige
    `bash scripts/prepare-sidecar.sh` **avant** le build, sinon on rebundle l'ancien binaire
    dans une app neuve et rien ne change a l'ecran.
  - **Second goulot, non corrige, cote broker → abonne** : `max_inflight_count = 100` dans
    `iakahub/rumqttd.toml`. Gimli a constate empiriquement que la redistribution a un abonne
    **cesse durablement** au-dela du seuil. Le tray recoit 227 messages par tick : il est en
    plein dans la zone a risque. C'est le pendant exact du bug qu'on vient de corriger et cela
    merite son propre cadrage. (Le meme plafond, plus `max_outgoing_packet_count = 200`, fausse
    aussi tout sniffer MQTT maison : des totaux pile a 100 ou 200 sont des artefacts de
    transport, pas des inventaires.)
  - **iakahub ne persiste pas les retained sur disque.** Tout redemarrage de l'app repart d'un
    broker vide. C'est ce qui explique les timestamps de 13 jours observes avant l'intervention :
    iakahub tournait sans interruption depuis le 2 septembre et les topics orphelins
    s'accumulaient faute de redemarrage.
  - **L'etat du tray est purement additif** : un payload vide fait echouer `parse_payload`
    (`src-tauri/src/state.rs`) et le message est ignore — ce qui le rend robuste a une purge,
    mais signifie qu'**aucun code deja appris n'est jamais oublie**. Si le daemon meurt, l'icone
    garde ses jauges indefiniment. La webview a des seuils de fraicheur (`FRESHNESS_5H`,
    `FRESHNESS_7D` dans `src/render.ts`), **l'icone du tray n'en a pas**.
  - **Remotes non conformes a la methode** : `origin` → `192.168.1.139:3001`, `iakabox` →
    `192.168.2.11:3001` (deux Forgejo LAN a des adresses differentes), plus un remote `github`
    vers `github.com/iakasju/iakaTokenCounter.git`. **Aucun remote VPS `git.naonedge.com`**,
    pourtant remote par defaut de la methode. Seul `origin` a ete pousse.

## Journal (versions & pauses)

| Date | Motif | Version | Branche | Note |
|---|---|---|---|---|
| 2026-09-17 00:54 | pause | v0.1.0 | main | Correctif transport MQTT sans perte (lot A + point 9) : 227 emis / 227 recus au lieu de 65, quota de nouveau publie, app rebuildee et reinstallee, recette a froid validee |
