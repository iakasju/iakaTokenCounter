# Instruction : <Titre de la feature ou du fix>

> Rédigé par Cowork. Consommé par Claude Code comme instruction de travail.
> Copier ce gabarit en `feature-xxx.md` (ou `fix-xxx.md`) et le remplir.

---

## Contexte

<!-- Pourquoi ce chantier ? Quel besoin ou quel problème ? -->

## Ce qui existe

| Élément | Où | État |
|---|---|---|
| <!-- --> | <!-- fichier/fonction --> | <!-- implémenté / partiel / absent --> |

## Décision

<!-- L'approche retenue et POURQUOI (les alternatives écartées et leur raison).
     C'est la partie la plus importante : elle évite que Claude Code parte sur la
     première solution qui compile. -->

## Étapes d'implémentation

1. <!-- étape concrète, vérifiable -->
2. <!-- ... -->
3. <!-- ... -->

## Fichiers concernés

- `src/...` — <!-- ce qui change -->

## Comportement attendu

- <!-- critère observable de réussite 1 -->
- <!-- critère observable de réussite 2 -->

## Vérification

- [ ] Typecheck OK
- [ ] Lint OK
- [ ] Tests ajoutés/à jour et verts
- [ ] Testé dans l'app réelle par le développeur

## Hors scope

- <!-- ce que cette instruction ne traite explicitement PAS -->
