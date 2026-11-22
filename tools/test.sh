#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$root/packages/loader-ios"
clang -Wall -Wextra -Werror -g -fsanitize=address,undefined tests/phoenix-wire-test.c tests/support/wire-parser.c -o "$work/phoenix-wire-test"
"$work/phoenix-wire-test"
cc -O1 -DLUA_USE_MACOSX -w -o "$work/lua" "$root/vendor/lua/onelua.c" -lm
for plugin in "$root"/plugins/*/; do
  for test in "$plugin"tests/*.lua; do
    [ -e "$test" ] || continue
    "$work/lua" "$test" "$plugin/main.lua"
  done
done
