#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
target="${1:?usage: stage-resources.sh <directory>}"
rm -rf "$target"
mkdir -p "$target/plugins" "$target/themes" "$target/assets" "$target/catalog/plugins" "$target/catalog/themes"
for plugin in "$root"/plugins/*/; do
  id="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["id"])' "$plugin/manifest.json")"
  for destination in "$target/plugins/$id" "$target/catalog/plugins/$id"; do
    mkdir -p "$destination"
    cp "$plugin/manifest.json" "$plugin/main.lua" "$plugin/signature" "$destination/"
  done
done
cp "$root"/themes/*.json "$target/themes/"
cp "$root"/catalog/index.json "$root"/catalog/index.json.sig "$target/catalog/"
cp "$root"/catalog/themes/*.json "$target/catalog/themes/"
cp "$root"/assets/*.svg "$target/assets/"
