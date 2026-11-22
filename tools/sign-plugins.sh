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
  manifest_digest="$(shasum -a 256 "$plugin/manifest.json" | awk '{ print $1 }')"
  source_digest="$(shasum -a 256 "$plugin/main.lua" | awk '{ print $1 }')"
  printf 'astral-plugin-v1\n%s\n%s\n' "$manifest_digest" "$source_digest" >"$work/message"
  sign "$work/message" >"$plugin/signature"
done
sign "$root/catalog/index.json" >"$root/catalog/index.json.sig"
