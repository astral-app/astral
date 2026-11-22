#!/bin/bash
set -euo pipefail
source="${1:?usage: embed-prelude.sh <prelude.lua> <header>}"
header="${2:?usage: embed-prelude.sh <prelude.lua> <header>}"
mkdir -p "$(dirname "$header")"
if grep -q ')astral_prelude"' "$source"; then
  echo "embed-prelude.sh: $source contains the raw string delimiter" >&2
  exit 1
fi
{
  printf 'static const char kAstralPrelude[] = R"astral_prelude('
  cat "$source"
  printf ')astral_prelude";\n'
} >"$header.tmp"
if cmp -s "$header.tmp" "$header"; then rm "$header.tmp"; else mv "$header.tmp" "$header"; fi
