#!/usr/bin/env bash
# Prepare le binaire sidecar attendu par Tauri (bundle.externalBin) : build du daemon
# `iakatc-daemon` en release + copie sous `src-tauri/binaries/iakatc-daemon-<target-triple>`.
# Tauri exige le suffixe target-triple (cf. specs/instructions/feature-tray-jauges.md D1).
# Le binaire est un artefact de build (gitignore) : rejouer ce script apres un clone.
set -euo pipefail

cd "$(dirname "$0")/.."

TARGET="${1:-$(rustc -vV | sed -n 's/host: //p')}"
echo "target-triple: $TARGET"

cargo build --release -p iakatc-daemon

DEST="src-tauri/binaries"
mkdir -p "$DEST"
cp "target/release/iakatc-daemon" "$DEST/iakatc-daemon-$TARGET"
echo "sidecar pret : $DEST/iakatc-daemon-$TARGET"
