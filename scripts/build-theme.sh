#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
(cd themes/emerald && bun install --frozen-lockfile && bun run build)
mkdir -p web/themes/emerald
rm -rf web/themes/emerald/dist
cp -R themes/emerald/dist web/themes/emerald/
cp themes/emerald/komari-theme.json themes/emerald/LICENSE web/themes/emerald/
