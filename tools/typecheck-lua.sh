#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
logs="$(mktemp -d)"
trap 'rm -rf "$logs"' EXIT
output="$(lua-language-server --check="$root" --configpath="$root/.luarc.json" --checklevel=Warning --logpath="$logs" 2>&1 | tr '\r' '\n')"
if ! grep -q "no problems found" <<<"$output"; then
  grep -E "\[(Warning|Error)\]" <<<"$output" || echo "$output"
  exit 1
fi
