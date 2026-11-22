#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
key="${ASTRAL_SIGNING_KEY_FILE:-$HOME/Library/Mobile Documents/com~apple~CloudDocs/astral-plugin-signing/astral-plugin-signing.pem}"
openssl="$(command -v /opt/homebrew/opt/openssl@3/bin/openssl || command -v openssl)"
[ -f "$key" ] || {
  echo "signing key not found: $key" >&2
  exit 1
}
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
sign() {
  "$openssl" pkeyutl -sign -rawin -inkey "$key" -in "$1" | base64 | tr -d '\n'
}
for plugin in "$root"/plugins/*/; do
  main="$(python3 -c 'import sys, tomllib; print(tomllib.load(open(sys.argv[1], "rb")).get("main", "main.lua"))' "$plugin/manifest.toml")"
  (
    cd "$plugin"
    {
      echo manifest.toml
      echo "$main"
      find i18n -name '*.toml' 2>/dev/null
    } | sort -u | while read -r path; do
      printf '%s %s\n' "$path" "$(shasum -a 256 "$path" | awk '{ print $1 }')"
    done
  ) >"$work/files"
  {
    echo "astral-plugin-v2"
    cat "$work/files"
  } >"$work/message"
  {
    echo "signature = \"$(sign "$work/message")\""
    echo
    echo "[files]"
    while read -r path digest; do echo "\"$path\" = \"$digest\""; done <"$work/files"
  } >"$plugin/signature"
done
sign "$root/catalog/index.toml" >"$root/catalog/index.toml.sig"
