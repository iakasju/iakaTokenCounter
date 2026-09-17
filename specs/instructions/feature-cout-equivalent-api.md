# Instruction : Coût équivalent API par modèle + tableau de bord de portefeuille

> Rédigé par Gandalf (P1 — cadrage). Consommé par Gimli (Claude Code) comme instruction de
> travail. Doc en français, code/identifiants en anglais.
> **Lot L2** du jalon arbitré par le décideur le 2026-09-17 (L0 + L1 + L2, **6 j**). L2 est estimé
> à **2 j** : 1,5 j pour le coût, **+0,5 j pour la bascule en tableau de bord de portefeuille**
> (arbitrage Q2), intégrée ici parce que c'est ce lot qui touche l'affichage.
> **Dépend de L0** (`feature-verite-des-chiffres.md`) : sans lui, tout montant affiché est faux
> d'un facteur ≈ 1,85.
>
> **Périmètre : `iakatc-core` (lecture seule) + webview.** Aucune modification du contrat MQTT ni
> du daemon.

---

## Contexte

Deux demandes du décideur convergent dans ce lot.

**1. Rendre le coût lisible.** `message.model` est présent sur chaque tour d'assistant des
transcripts, et **personne ne le lit** — ni le contrat MQTT, ni `history.rs`, ni aucune
visualisation. C'est pourtant la dimension qui transforme un comptage de tokens en une grandeur
décidable : les tarifs diffèrent d'un facteur 10 entre modèles, et d'un facteur 10 encore entre un
token d'entrée frais et un token lu en cache. Sans elle, « 9,2 milliards de tokens » ne veut rien
dire ; avec elle, cela devient un montant. Répartition réelle mesurée sur 45 jours (comptage brut,
à ramener au réel par le facteur de L0) :

| Modèle | Entrée fraîche | Sortie | Cache read |
|---|---|---|---|
| `claude-opus-5` | 317 214 | 34 137 004 | 9 932 187 464 |
| `claude-sonnet-5` | 52 926 | 6 411 591 | 5 491 170 217 |
| `claude-fable-5-1` | 49 428 | 3 467 445 | 732 021 865 |
| `claude-opus-4-7`, `-4-8`, `claude-haiku-4-5` | résiduels | résiduels | résiduels |

**2. Cesser de regarder un seul provider à la fois.** La demande d'origine parlait de
« l'utilisation des ia dans iaka », **au pluriel**. La fenêtre, elle, est aujourd'hui scopée à un
provider (D4 de `feature-app-analytics.md`). Le décideur a arbitré : **tableau de bord de
portefeuille**, tous comptes et tous providers dans une vue unique, comparables entre eux.

## Faits vérifiés (tarifs, 2026-09-17)

Tarifs publics par million de tokens (entrée / sortie) : **Opus 5** 5 \$ / 25 \$ · **Sonnet 5**
2 \$ / 10 \$ · **Fable 5.1** 10 \$ / 50 \$ · **Sonnet 4.6** 3 \$ / 15 \$ · **Haiku 4.5** 1 \$ / 5 \$ ·
**Opus 4.6 / 4.7 / 4.8** 5 \$ / 25 \$.

Coûts du cache : lecture ≈ **0,1×** le tarif d'entrée, écriture ≈ **1,25×** en TTL 5 min et ≈ **2×**
en TTL 1 h.

> ⚠️ **Les taux de cache ne sont PAS uniformes.** Fable 5.1 facture la lecture de cache à un tarif
> propre (0,25 \$/M), qui n'est pas 0,1× son entrée. **La table doit donc porter des taux
> explicites par modèle** — entrée, sortie, lecture de cache, écriture 5 min, écriture 1 h — et
> **jamais un multiplicateur global appliqué à tout le monde**. C'est la première façon dont ce lot
> peut produire un chiffre faux et confiant.

**Les transcripts portent le détail nécessaire** : `usage.cache_creation.ephemeral_5m_input_tokens`
et `ephemeral_1h_input_tokens` distinguent les deux TTL d'écriture de cache — la ventilation fine
est donc possible sans approximation.

## Ce qui existe (à réutiliser)

| Élément | Où | Rôle pour ce lot |
|---|---|---|
| Scans récursifs et dédupliqués | `iakatc-core/src/measure/claude.rs` **après L0** | Source des volumes justes |
| Bloc `usage` complet | transcripts : `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens`, `cache_creation.ephemeral_{5m,1h}_input_tokens` | Matière du calcul |
| Grandeurs nommées | L0, D3 (« Travail » / « Volume total ») | « Volume total » passe en second rang ici |
| En-tête quota **déjà multi-comptes** | `src-tauri/src/state.rs::get_reservoirs:252` renvoie **toutes** les cartes | La donnée portefeuille est déjà là |
| Filtre mono-compte à retirer | `src/analytics.ts::renderQuota:42` (`find` sur `(provider, account)`) | Le seul verrou de la vue mono-compte |
| Commande d'historique | `src-tauri/src/history.rs::get_history:107` (paramètre `provider`) | À élargir au portefeuille |
| Visualisations maison SVG | `src/history.ts` | À enrichir, sans lib de charting (D5 d'origine) |

## Décision

### D1 — La table de tarifs est **une donnée nommée et isolée, datée à l'écran**

**Retenu** : un fichier de données dédié — `iakatc-core/src/pricing.rs` (ou un JSON embarqué par
`include_str!`) — qui porte, **par identifiant de modèle**, cinq taux explicites :

```
model_id → { input, output, cache_read, cache_write_5m, cache_write_1h }   en $ / million de tokens
+ un champ de portée : { valid_as_of: "AAAA-MM-JJ", source: "<url>" }
```

**Deux exigences non négociables**, inscrites à la demande du coordinateur et que je fais miennes :

1. **Donnée nommée et isolée, jamais des littéraux dispersés.** Un tarif en dur au milieu d'un
   calcul est introuvable le jour où il change — et il changera.
2. **L'écran affiche la date de validité des tarifs.** *Un montant sans date de tarif est un
   montant invérifiable*, et c'est précisément le genre de chiffre faux et confiant que ce jalon
   existe pour éliminer. Le libellé doit être visible avec le montant, pas caché dans un menu :
   « équivalent API, tarifs au AAAA-MM-JJ ».

**Modèle inconnu de la table** : **ne pas deviner, ne pas extrapoler depuis un modèle voisin.** Ses
tokens sont comptés dans les volumes et **exclus du montant**, avec une mention explicite du type
« N tours non tarifés (modèle inconnu : `<id>`) ». Un montant partiel annoncé comme tel vaut mieux
qu'un montant complet inventé.

> **Micro-choix tranché** : pas de récupération automatique des tarifs depuis le réseau. La
> maintenance manuelle est **assumée** par le décideur (arbitrage Q3). Une mise à jour = une ligne
> de données + une date.

### D2 — Le coût se calcule au **tour**, jamais sur des agrégats

**Retenu** : le coût d'un tour est calculé **au moment du fold**, avec le tarif du modèle **de ce
tour**, puis agrégé. Jamais l'inverse.

**Pourquoi** : agréger d'abord les tokens puis appliquer un tarif « moyen » suppose un mix de
modèles homogène — il ne l'est pas (Opus et Sonnet se partagent la charge dans des proportions qui
varient par projet et par jour). Ce serait réintroduire, dans le calcul, exactement le genre
d'approximation que L0 vient de retirer des comptages.

**Décomposition à conserver** jusqu'à l'affichage, car elle est la matière de la statistique S9 :
**lecture de cache / écriture de cache / entrée fraîche / sortie**. Avec 621 M de tokens d'écriture
de cache à un tarif supérieur à l'entrée fraîche, ce poste pèse probablement plus lourd que la
sortie du modèle — un fait aujourd'hui totalement invisible.

### D3 — « Équivalent API », jamais « dépense »

Le décideur est sur abonnement : **son coût marginal est nul**. Le montant affiché est donc une
**valeur de référence** — ce que la même consommation aurait coûté au tarif à l'usage.

**Le libellé retenu est « équivalent API »**, et la formulation de la valeur est **« votre
abonnement vous a évité ≈ X \$ »**, jamais « vous avez dépensé X \$ ». La seconde serait
factuellement fausse.

### D4 — La vue devient un **tableau de bord de portefeuille**

**Retenu** : une vue unique montrant **tous les comptes et tous les providers**, comparables entre
eux. Concrètement :

- **En-tête** : toutes les cartes de réservoir renvoyées par `get_reservoirs` — le filtre
  `find((provider, account))` de `renderQuota:42` disparaît. **La donnée est déjà là**, la commande
  renvoie déjà toutes les cartes : c'est un filtre à retirer, pas une source à créer.
- **Corps** : l'historique devient l'**union des providers**. `get_history(provider)` devient
  `get_history(scope)` où `scope` vaut un provider **ou** « tout le portefeuille ».
- **Le provider reste une dimension de lecture** (couleur, filtre, ventilation), il cesse d'être un
  périmètre imposé.

**Ce que devient le double-clic — la question posée par le coordinateur.** `open_analytics(provider,
account)` **garde tout son sens, mais change de sémantique** : ses arguments cessent d'être un
**filtre** pour devenir une **mise en évidence**. La fenêtre s'ouvre sur le tableau de bord complet,
avec le compte double-cliqué **pré-sélectionné et visuellement distingué**.

**Pourquoi conserver la signature** : (a) aucune rupture côté tray, qui connaît déjà le couple et
l'envoie ; (b) le geste garde sa promesse — on double-clique *sur un compte*, on s'attend à voir
*ce compte* — tout en montrant désormais son contexte ; (c) une signature sans argument obligerait
l'utilisateur à retrouver son compte dans la liste, ce qui dégraderait le geste.

**Écarté** : ouvrir deux vues distinctes (une par compte, une portefeuille) — deux interfaces à
maintenir pour une seule question ; et supprimer les arguments — perte de l'intention du geste.

> **Le bandeau de portée de D4 d'origine doit être réécrit, pas supprimé.** La limitation
> `account_ambiguous` **reste vraie et devient plus visible** dans une vue portefeuille : le
> **quota** est par compte, la **consommation** ne l'est pas — elle est par provider, faute
> d'identifiant de compte dans les transcripts. Dans une vue qui affiche les comptes côte à côte,
> ne pas le dire serait laisser croire à une ventilation par compte qui n'existe pas.

### D5 — Codex : volumes oui, montant non

**Retenu** : Codex figure dans le tableau de bord avec ses **volumes** et son **quota**, mais
**sans montant**, avec la mention « coût indisponible pour ce provider ».

**Pourquoi** : les rollouts Codex n'exposent pas d'identifiant de modèle exploitable de la même
façon que `message.model`, et les tarifs d'un autre fournisseur ne sont pas dans la table. Inventer
une équivalence serait produire le chiffre faux et confiant que ce jalon combat.

> **Tranché sans remonter au décideur** (sa question Q5) : Codex **n'est pas exclu de la vue** — ce
> serait absurde dans un tableau de bord de portefeuille — il est **présent sans montant**. Aligner
> Codex sur le coût reste une extension possible, non engagée, et suppose d'abord d'établir quel
> modèle a servi. Aucune modification de `codex.rs` dans ce lot.

---

## Étapes d'implémentation

1. **Table de tarifs** (D1) : `iakatc-core/src/pricing.rs` — structure `ModelPricing` à cinq taux,
   table par identifiant de modèle, champs `valid_as_of` et `source`. Test : un modèle inconnu
   renvoie `None`, il n'est **jamais** rabattu sur un voisin.
2. **Axe modèle dans les scans** : étendre les accumulateurs de `claude.rs` pour porter
   `message.model` comme dimension, et distinguer les deux TTL d'écriture de cache
   (`ephemeral_5m` / `ephemeral_1h`). Étendre les fixtures en conséquence.
3. **Calcul du coût au tour** (D2) : une fonction pure
   `cost_of(usage, model, &table) -> Option<CostBreakdown>` avec la décomposition lecture de cache
   / écriture de cache / entrée fraîche / sortie. Testée sur des valeurs calculées à la main.
4. **Agrégation** : coût par projet, par modèle, et croisement projet × modèle ; total global, plus
   le compte des **tours non tarifés**.
5. **Commande élargie** (D4) : `get_history(scope)` acceptant un provider ou le portefeuille
   entier ; charge utile enrichie du coût et de la ventilation par modèle.
6. **En-tête portefeuille** (D4) : retirer le filtre de `renderQuota:42`, afficher toutes les
   cartes, **distinguer visuellement** le compte passé en argument.
7. **Bandeau de portée réécrit** (D4) : quota par compte, consommation par provider —
   `account_ambiguous` énoncé pour une vue multi-comptes.
8. **Affichage du coût** : montant global « votre abonnement vous a évité ≈ X \$ » (D3), classement
   des projets par coût, ventilation par modèle, décomposition du poste cache — **avec la date de
   validité des tarifs visible à côté du montant** (D1).
9. **Codex sans montant** (D5) : mention explicite, jamais une case vide ni un zéro.
10. **Rollups** : renseigner le champ `model` prévu par L1 (D4 de L1), désormais disponible.

## Fichiers concernés

- `iakatc-core/src/pricing.rs` — **nouveau** : table de tarifs, `valid_as_of`, calcul de coût pur.
- `iakatc-core/src/measure/claude.rs` — axe `model`, TTL d'écriture de cache, agrégats de coût.
- `iakatc-core/src/lib.rs` — déclaration du module `pricing`.
- `src-tauri/src/history.rs` — `get_history(scope)`, charge utile enrichie.
- `src-tauri/src/state.rs` — `open_analytics` : documenter la sémantique « mise en évidence »
  (signature inchangée).
- `src/analytics.ts` — en-tête portefeuille, bandeau de portée réécrit, mise en évidence du compte.
- `src/history.ts` — affichage des montants, ventilation par modèle, décomposition du cache.
- `src/types.ts` — types de coût et de ventilation.
- `src-tauri/src/rollups.rs` — champ `model` renseigné.
- Tests : `pricing.rs` (calculs, modèle inconnu), `claude.rs` (axe modèle), front (libellés, date).

## Comportement attendu

- [ ] Le tableau de bord affiche **tous les comptes et tous les providers**, sans double-clic
      préalable sur chacun.
- [ ] Un double-clic sur la carte `(claude, max)` ouvre la vue **complète** avec ce compte
      **mis en évidence** — pas une vue filtrée sur lui.
- [ ] Le montant global est libellé **« équivalent API »** et formulé en **économie réalisée**,
      jamais en dépense (D3).
- [ ] **La date de validité des tarifs est affichée à côté du montant** (D1). Son absence est un
      échec du lot, pas un détail cosmétique.
- [ ] Le classement des projets **par coût** diffère du classement **par tokens** — c'est le signe
      que le calcul au tour fonctionne et que l'axe modèle sert à quelque chose.
- [ ] La ventilation par modèle retrouve l'ordre de grandeur mesuré : **Opus 5 majoritaire**,
      Sonnet 5 en second, Fable 5.1 en troisième.
- [ ] Un modèle absent de la table produit une **mention explicite de tours non tarifés**, et
      **aucun montant inventé**.
- [ ] Codex apparaît avec ses volumes et son quota, et la mention **« coût indisponible pour ce
      provider »** (D5).
- [ ] Le bandeau de portée énonce que **le quota est par compte et la consommation par provider**.
- [ ] Ordre de grandeur attendu sur 45 jours, après L0 : **≈ 6 100 – 7 400 \$**. Un résultat proche
      de 3 500 \$ signale que L0 n'est pas effectif ; un résultat proche de 13 000 \$ signale que la
      déduplication ne l'est pas.

## Vérification

- [ ] `cargo check` / typecheck front OK
- [ ] `cargo clippy` + lint front OK
- [ ] `cargo test` vert (tarifs, modèle inconnu, coût au tour calculé à la main, axe modèle)
- [ ] `bash scripts/quality-report.sh` OK
- [ ] Testé dans l'app réelle : double-clic depuis deux comptes différents, montant et date
      affichés, Codex sans montant, ordre de grandeur conforme

## Hors scope

- **Le coût de Codex** (D5) — présent sans montant, extension non engagée.
- **La récupération automatique des tarifs** depuis le réseau (D1) : maintenance manuelle assumée.
- **La conversion en euros** ou toute autre devise : le tarif de référence est en dollars.
- **Les visualisations de quota dans le temps** (courbe d'épuisement, projection, comparaison
  historique des comptes) → lot **L4, non engagé**. Ce lot affiche le quota **courant** de tous les
  comptes, pas son évolution.
- **L'axe persona** (quel agent coûte le plus) → lot **L5, non engagé**.
- **Toute lib de charting** : visualisations maison SVG (D5 de `feature-app-analytics.md`).
- **Toute modification du contrat MQTT, des topics, du daemon.**

## Sources

- Proposition et arbitrage : `specs/instructions/proposition-analytics-riche.md` (§ 4.4 axe modèle
  et ordres de grandeur, § 6 grandeurs, § 8.3 pourquoi pas une « dépense », Q2 et Q3).
- Cadrage d'origine : `specs/instructions/feature-app-analytics.md` (D4 portée, D5 viz maison).
- Lot prérequis : `specs/instructions/feature-verite-des-chiffres.md`.
- Tarifs par modèle et coûts relatifs du cache : référentiel `claude-api` et
  [tarification Anthropic](https://www.anthropic.com/pricing) — **à redater à l'implémentation**,
  et à reporter dans le champ `valid_as_of` de la table.
