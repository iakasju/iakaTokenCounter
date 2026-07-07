#!/usr/bin/env bash
# Rapport qualité consolidé — iakaTokenCounter (Rust / Cargo workspace).
# Gate à passer avant de considérer une tâche finie / avant intégration.
# Stack : workspace Cargo (iakatc-core + iakatc-daemon). Cf. specs/instructions/feature-collecteur-logs.md.
set -uo pipefail

cd "$(dirname "$0")/.." || exit 2

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

run "Build (check)" cargo check --workspace --all-targets
run "Lint (clippy)" cargo clippy --workspace --all-targets -- -D warnings
run "Tests"         cargo test --workspace

if [ "$fail" -eq 0 ]; then
  echo "VERDICT: PASS ✅"
else
  echo "VERDICT: FAIL ❌"
fi
exit "$fail"
