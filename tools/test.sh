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
  python3 - "$plugin" "$work/message" "$work/signature" <<'PYTHON'
import hashlib, pathlib, sys, tomllib
folder = pathlib.Path(sys.argv[1])
document = tomllib.loads((folder / "signature").read_text())
files = document["files"]
main = tomllib.loads((folder / "manifest.toml").read_text()).get("main", "main.lua")
expected = {"manifest.toml", main} | {f"i18n/{path.name}" for path in (folder / "i18n").glob("*.toml")}
if set(files) != expected:
    sys.exit(f"{folder.name}: signature lists {sorted(set(files) ^ expected)} differently from the folder (run tools/sign-plugins.sh)")
for path, digest in files.items():
    if hashlib.sha256((folder / path).read_bytes()).hexdigest() != digest:
        sys.exit(f"{folder.name}: {path} changed since it was signed (run tools/sign-plugins.sh)")
pathlib.Path(sys.argv[2]).write_text("astral-plugin-v2\n" + "".join(f"{path} {files[path]}\n" for path in sorted(files)))
pathlib.Path(sys.argv[3]).write_text(document["signature"])
PYTHON
  "$work/verify-signature" "$work/message" "$work/signature" "$key" || {
    echo "invalid signature: $plugin (run tools/sign-plugins.sh)" >&2
    exit 1
  }
done
"$work/verify-signature" "$root/catalog/index.toml" "$root/catalog/index.toml.sig" "$key" || {
  echo "stale or invalid catalog signature (run tools/sign-plugins.sh)" >&2
  exit 1
}
echo "signatures ok"
if [ -n "${ASTRAL_PHOENIX_BINARY:-}" ]; then
  clang -Wall -Wextra -Werror -O1 "$root/packages/loader-ios/tests/locate-test.c" "$root/packages/loader-ios/sources/PhoenixLocate.c" -o "$work/locate-test"
  "$work/locate-test" "$ASTRAL_PHOENIX_BINARY"
fi
