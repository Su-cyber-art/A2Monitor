#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
npm --prefix web ci
npm --prefix web run build
sh scripts/build-theme.sh
cargo build --release --locked --workspace
ARCH=$(uname -m)
SYSTEM=$(uname -s | tr '[:upper:]' '[:lower:]')
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT HUP INT TERM
mkdir -p "$STAGE/bin" "$STAGE/web" artifacts
cp target/release/monitor-hub target/release/monitor-agent "$STAGE/bin/"
cp -R web/dist "$STAGE/web/"
mkdir -p "$STAGE/web/themes/emerald"
cp -R themes/emerald/dist "$STAGE/web/themes/emerald/"
cp themes/emerald/komari-theme.json themes/emerald/LICENSE "$STAGE/web/themes/emerald/"
cp -R scripts docs LICENSE "$STAGE/"
if [ -f README.md ]; then cp README.md "$STAGE/"; fi
COPYFILE_DISABLE=1 tar -czf "artifacts/monitor-$SYSTEM-$ARCH.tar.gz" -C "$STAGE" .
echo "artifacts/monitor-$SYSTEM-$ARCH.tar.gz"
