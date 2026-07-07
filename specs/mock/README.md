# Mocks — jeu de fixtures hors-ligne (daemon v0)

Fixtures pour tester le daemon **sans broker reel ni vrais logs**. Aucun test n'exige le
broker ni de vraies sessions : les tests unitaires embarquent leurs cas ; ces fichiers servent
aux tests **manuels de recette** (§ Verification de l'instruction) et de reference de forme.

## Contenu

| Fichier | Sert a |
|---|---|
| `codex_rollout_sample.jsonl` | **Rollout Codex reel** (session_meta + `token_count`) — shape confirmee 0.142.3. Utilise par le test `measure::codex::fixture_reelle_*`. |
| `claude_projects/-w-alpha/session-alpha.jsonl` | Transcript Claude : 1 tour **coordinateur** + 1 tour **sous-agent** (`isSidechain`) sur le projet `alpha`. |
| `claude_projects/-w-beta/session-beta.jsonl` | 2e projet `beta` + 1 **ligne corrompue** (doit etre ignoree sans crash). |
| `quota/claude.max.json` | Fenetres 5h **et** 7d **fraiches** (`captured_at`/`resets_at` futurs) -> `official`. |
| `quota/claude.pro.json` | 5h **capturee anciennement** mais `resets_at` futur -> `official_stale`. |
| `quota/claude.bad.json` | Fichier **malforme** -> ignore avec warning, sans faire echouer le tick. |
| `config.json` | Seuils de fraicheur + plafonds (compte `demo` avec plafonds, `default`/`codex` a `null`). |

## Usage — tick manuel hors broker

Pointer le daemon sur ces fixtures (les scanners lisent `~/.claude/projects` et
`$IAKATC_HOME/quota`) :

```bash
# 1. Preparer un IAKATC_HOME jetable avec le quota + la config
export IAKATC_HOME="$(mktemp -d)"
cp -r specs/mock/quota "$IAKATC_HOME/"
cp specs/mock/config.json "$IAKATC_HOME/"

# 2. Faire pointer Claude Code / Codex sur les transcripts mock (HOME jetable)
export HOME_MOCK="$(mktemp -d)"
mkdir -p "$HOME_MOCK/.claude"
cp -r specs/mock/claude_projects "$HOME_MOCK/.claude/projects"

# 3. Lancer un tick (broker absent = degrade propre, le daemon publie quand meme meta/*)
HOME="$HOME_MOCK" IAKATC_TICK_SECONDS=1 cargo run -p iakatc-daemon
```

## Usage — capture statusline

```bash
echo '{"version":"2.1.90","rate_limits":{"five_hour":{"used_percentage":23.5,"resets_at":4102448400}}}' \
  | IAKATC_ACCOUNT_LABEL=max cargo run -p iakatc-daemon -- statusline-capture
# -> ecrit $IAKATC_HOME/quota/claude.max.json, imprime "iakatc max 5h:24%", code 0.
```
