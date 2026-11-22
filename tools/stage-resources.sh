#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
target="${1:?usage: stage-resources.sh <directory>}"
rm -rf "$target"
mkdir -p "$target/plugins" "$target/themes" "$target/assets" "$target/catalog/plugins" "$target/catalog/themes"
for plugin in "$root"/plugins/*/; do
  id="$(python3 -c 'import tomllib,sys; print(tomllib.load(open(sys.argv[1], "rb"))["id"])' "$plugin/manifest.toml")"
  for destination in "$target/plugins/$id" "$target/catalog/plugins/$id"; do
    mkdir -p "$destination"
    cp "$plugin/manifest.toml" "$plugin/main.lua" "$plugin/signature" "$destination/"
    if [ -d "$plugin/i18n" ]; then cp -R "$plugin/i18n" "$destination/"; fi
  done
done
cp "$root"/themes/*.toml "$target/themes/"
cp "$root"/catalog/index.toml "$root"/catalog/index.toml.sig "$target/catalog/"
cp "$root"/catalog/themes/*.toml "$target/catalog/themes/"
cp "$root"/assets/*.svg "$target/assets/"
