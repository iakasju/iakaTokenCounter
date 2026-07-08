#!/usr/bin/env bash
# Prepare les binaires sidecar attendus par Tauri (bundle.externalBin) :
#   - `iakahub`        : le backbone spawne par la GUI (broker MQTT local + orchestration) ;
#   - `iakatc-daemon`  : le measure daemon, co-embarque a cote (iakahub le localise et le lance).
# Les deux sont builds en release puis copies sous `src-tauri/binaries/<nom>-<target-triple>`.
# Tauri exige le suffixe target-triple (cf. specs/instructions/feature-tray-jauges.md D1 et
# feature-iakahub.md D4). iakahub trouve `iakatc-daemon` a cote de son propre executable : les
# deux binaires doivent donc etre dans le meme dossier (bundle ou target/), garanti ici.
# Les binaires sont des artefacts de build (gitignore) : rejouer ce script apres un clone.
set -euo pipefail

cd "$(dirname "$0")/.."

TARGET="${1:-$(rustc -vV | sed -n 's/host: //p')}"
echo "target-triple: $TARGET"

cargo build --release -p iakahub -p iakatc-daemon

DEST="src-tauri/binaries"
mkdir -p "$DEST"

for bin in iakahub iakatc-daemon; do
  cp "target/release/$bin" "$DEST/$bin-$TARGET"
  echo "sidecar pret : $DEST/$bin-$TARGET"
done
