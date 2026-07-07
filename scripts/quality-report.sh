#!/usr/bin/env bash
# Rapport qualité consolidé — iakaTokenCounter (Node.js / TypeScript)
# Gate à passer avant de considérer une tâche finie / avant intégration.
set -uo pipefail

fail=0
run() {
  local label="$1"; shift
  echo "── $label ─────────────────────────────────────────"
  if "$@"; then
    echo "  ✅ $label OK"
  else
    echo "  ❌ $label ÉCHEC"
    fail=1
  fi
  echo
}

# Décommenter au fur et à mesure que les scripts npm existent (voir package.json).
run "Typecheck" npm run --silent typecheck
run "Lint"      npm run --silent lint
run "Tests"     npm run --silent test

if [ "$fail" -eq 0 ]; then
  echo "VERDICT: PASS ✅"
else
  echo "VERDICT: FAIL ❌"
fi
exit "$fail"
