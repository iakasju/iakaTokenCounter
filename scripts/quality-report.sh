#!/usr/bin/env bash
# Rapport qualité consolidé — iakaTokenCounter.
# Gate à passer avant de considérer une tâche finie / avant intégration.
# Stack : workspace Cargo (iakatc-core + iakatc-daemon + src-tauri/GUI tray) + front TS (Vite).
# Cf. specs/instructions/feature-collecteur-logs.md et feature-tray-jauges.md.
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

# --- Front TS (GUI tray) : deps + typecheck + build du dist/ ---------------------------------
# Le crate `src-tauri` (tauri-build) exige `dist/` a la compilation → on le produit d'abord.
if [ ! -d node_modules ]; then
  run "Front (npm ci)" npm ci
fi
run "Front (typecheck)" npm run typecheck
run "Front (build dist)" npm run build

# --- Sidecar : le binaire daemon target-triple doit exister pour le bundling Tauri -----------
# (Sans lui, le compile passe mais `cargo tauri build` echoue au bundling — cf. prepare-sidecar.sh.)
if ! ls src-tauri/binaries/iakatc-daemon-* >/dev/null 2>&1; then
  run "Sidecar (build+copy)" bash scripts/prepare-sidecar.sh
fi

# --- Rust : workspace complet (core + daemon + tray) ----------------------------------------
run "Build (check)" cargo check --workspace --all-targets
run "Lint (clippy)" cargo clippy --workspace --all-targets -- -D warnings
run "Tests"         cargo test --workspace

if [ "$fail" -eq 0 ]; then
  echo "VERDICT: PASS ✅"
else
  echo "VERDICT: FAIL ❌"
fi
exit "$fail"
