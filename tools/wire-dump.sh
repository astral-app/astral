#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
device="${DEVICE:?set DEVICE to the test device UDID}"
pilot="${JB_PILOT:-$HOME/go/bin/jb-p1lot}"
output="${1:?usage: wire-dump.sh <output directory>}"
mkdir -p "$output"
tool="$output/.wire-dump"
clang -O1 -o "$tool" "$root/packages/loader-ios/tests/support/wire-dump.c" "$root/packages/loader-ios/tests/support/wire-parser.c"
state="$("$pilot" shell_exec --device "$device" --command 'ls -d /var/mobile/Containers/Data/Application/*/Documents/astral | head -1' | python3 -c 'import json,sys; print(json.load(sys.stdin)["output"].strip())')"
for file in $("$pilot" shell_exec --device "$device" --command "ls $state/wire" | python3 -c 'import json,sys; print(json.load(sys.stdin)["output"])'); do
  "$pilot" file_transfer --device "$device" --direction device_to_host --source "$state/wire/$file" --destination "$output/$file" >/dev/null
done
"$tool" "$output"/*.bin >"$output/wire.jsonl"
echo "$(wc -l <"$output/wire.jsonl") batches in $output/wire.jsonl"
