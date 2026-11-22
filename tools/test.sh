#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$root/packages/loader-ios"
clang -Wall -Wextra -Werror -g -fsanitize=address,undefined tests/phoenix-wire-test.c tests/support/wire-parser.c -o "$work/phoenix-wire-test"
"$work/phoenix-wire-test"
lua_platform="-DLUA_USE_LINUX"
[ "$(uname)" = "Darwin" ] && lua_platform="-DLUA_USE_MACOSX"
cc -O1 "$lua_platform" -w -o "$work/lua" "$root/vendor/lua/onelua.c" -lm
for plugin in "$root"/plugins/*/; do
  for test in "$plugin"tests/*.lua; do
    [ -e "$test" ] || continue
    "$work/lua" "$test" "$plugin/main.lua"
  done
done
cc -O1 -w -I"$root/vendor/monocypher/src" -I"$root/vendor/monocypher/src/optional" -o "$work/verify-signature" \
  "$root/packages/loader-ios/tests/support/verify-signature.c" "$root/vendor/monocypher/src/monocypher.c" \
  "$root/vendor/monocypher/src/optional/monocypher-ed25519.c"
key="$(cat "$root/keys/plugin-signing.pub")"
for plugin in "$root"/plugins/*/; do
  manifest_digest="$(sha256sum "$plugin/manifest.json" 2>/dev/null || shasum -a 256 "$plugin/manifest.json")"
  source_digest="$(sha256sum "$plugin/main.lua" 2>/dev/null || shasum -a 256 "$plugin/main.lua")"
  printf 'astral-plugin-v1\n%s\n%s\n' "${manifest_digest%% *}" "${source_digest%% *}" >"$work/message"
  "$work/verify-signature" "$work/message" "$plugin/signature" "$key" || {
    echo "stale or invalid signature: $plugin (run tools/sign-plugins.sh)" >&2
    exit 1
  }
done
"$work/verify-signature" "$root/catalog/index.json" "$root/catalog/index.json.sig" "$key" || {
  echo "stale or invalid catalog signature (run tools/sign-plugins.sh)" >&2
  exit 1
}
echo "signatures ok"
if [ -n "${ASTRAL_PHOENIX_BINARY:-}" ]; then
  clang -Wall -Wextra -Werror -O1 "$root/packages/loader-ios/tests/locate-test.c" "$root/packages/loader-ios/sources/PhoenixLocate.c" -o "$work/locate-test"
  "$work/locate-test" "$ASTRAL_PHOENIX_BINARY"
fi
