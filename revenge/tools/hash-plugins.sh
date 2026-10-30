#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../plugins"
for dir in */; do
  manifest="$dir/manifest.json"
  main=$(jq -r '.main // "index.js"' "$manifest")
  hash=$(sha256sum "$dir/$main" | cut -c1-16)
  jq --arg hash "$hash" '.hash = $hash' "$manifest" > "$manifest.tmp"
  mv "$manifest.tmp" "$manifest"
done
