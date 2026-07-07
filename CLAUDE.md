# CLAUDE.md — Instructions pour Claude Code

> Ce fichier est lu en priorité par Claude Code à chaque session.
> Pour la vision complète du projet, lire `specs/PROJET.md`.
> Pour la méthode de collaboration, voir `methode-de-travail.md` (iakaframe).

---

## Rôles (rappel)

- **Cadrage** (réflexion) rédige les instructions dans `specs/instructions/`. Il ne
  modifie jamais le code.
- **Claude Code** (toi) lis l'instruction correspondante AVANT chaque tâche, puis
  implémentes, builds, testes et commites.

---

## Ce qu'est ce projet

**iakaTokenCounter** : un **moniteur de consommation IA multi-comptes en tray**. Il
affiche dans la barre système une **jauge de quota restant par compte IA** (le contingent
du plan se recharge), alimentée par le **parsing des logs locaux des agents**. Un
**double-clic** ouvre une **app locale d'analytics** (historique/courbes par compte). App
**autonome** (sans dépendance iaka) mais à pertinence maximale dans iakaproject. Le
comptage de tokens est une **brique interne** (fallback de conso), pas le produit.

Stack : **multi-OS** (Tauri ou Electron — à trancher), **TypeScript**. Voir `specs/PROJET.md`.

---

## Commandes à utiliser

```bash
npm run dev          # exécuter en dev (tsx / ts-node)
npm run build        # compilation TypeScript -> dist/
npm run test         # tests unitaires
npm run lint         # eslint
npm run typecheck    # tsc --noEmit
bash scripts/quality-report.sh   # rapport qualité consolidé (typecheck + lint + test)
```

<!-- Ces commandes seront figées à la 1re instruction dev ; adapter le package.json en
     conséquence. -->

---

## Conventions

- **Langue du code** : anglais (identifiants, commits techniques).
- **Langue de la doc et des échanges** : français.
- **Commits** : *conventional commits* (`feat:`, `fix:`, `docs:`, `chore:`, `wip:`).
- **Commits atomiques et fréquents** : après chaque étape logique (filet de
  sécurité pour pouvoir revenir en arrière). Jamais de `reset --hard` ni de
  `push --force` de ton côté.
- **MVP d'abord, puis itérer.** Pas de sur-ingénierie.
- **Self-hosted / open-source d'abord** pour tout choix de backend ; cloud en
  fallback justifié seulement.
- **Réutiliser l'existant** (infra, services, MCP) avant de réimplémenter.
- En dev, **mocker les appels API** coûteux/limités (voir `specs/mock/`).

---

## Dépôt git : Forgejo (iakabox)

Remote par défaut : **Forgejo LAN** `http://192.168.2.11:3001/sjupin/iakaTokenCounter.git`,
**HTTP + token** (SSH inutilisable). Token via `$FORGEJO_TOKEN` ou `.git/config`
local — **jamais commité**. Voir `iakabox-usage.html` (iakaframe) pour clone/push,
création de dépôt (API, description **ASCII**) et rotation de token.

## Cycle de documentation (état des lieux)

Régénérer l'état des lieux **à chaque changement de version** et **à chaque pause /
préparation de reprise** (via la skill `iakaframe-etat-des-lieux` ou le script snapshot).
Génère `specs/etat-des-lieux.md`. **Compléter le récit de reprise** dans le `.md` (ce qui
vient d'être fait, ce qui reste, prochaine étape).

---

## Avant toute tâche non triviale

1. Lire l'instruction correspondante dans `specs/instructions/`.
2. Si elle n'existe pas → le signaler ; ne pas improviser une feature lourde sans
   spec. Proposer un plan court d'abord.
3. Implémenter étape par étape, avec commits intermédiaires.
4. Lancer typecheck + lint + tests avant de considérer la tâche finie.
5. Pour toute action vraiment destructive hors denylist : **demander confirmation
   par message texte avant d'agir.**

---

## Backlog

- [ ] Cœur de tokenisation Claude — `specs/instructions/feature-tokenizer-claude.md`
- [ ] CLI de comptage (`itc count <fichier|->`) — `specs/instructions/feature-cli.md`
- [ ] Estimation de coût par modèle — `specs/instructions/feature-cost-estimate.md`
- [ ] Multi-modèles (GPT, etc.) — `specs/instructions/feature-multi-model.md`
