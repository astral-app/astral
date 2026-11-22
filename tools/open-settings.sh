#!/bin/bash
set -uo pipefail
device="${DEVICE:?set DEVICE to the test device UDID}"
pilot="${JB_PILOT:-$HOME/go/bin/jb-p1lot}"
target="${1:-General}"
labels() { "$pilot" ui_snapshot --device "$device" 2>/dev/null | python3 -c "import json,sys; d=json.load(sys.stdin); [print((n.get('label') or '').replace(' ', '_'), int(n['activationPoint']['x']), int(n['activationPoint']['y'])) for n in d['nodes']]"; }
tap() { "$pilot" ui_action --device "$device" --action tap --x "$1" --y "$2" >/dev/null 2>&1; sleep "${3:-2}"; }
for attempt in 1 2 3 4; do
  snapshot="$(labels)"
  if grep -q '^Account_Settings ' <<<"$snapshot"; then
    row="$(grep -E "^${target// /_} [0-9]+ [0-9]+$" <<<"$snapshot" | tail -1)"
    [ -n "$row" ] && { set -- $row; tap 187 "$3" 3; labels; exit 0; }
  fi
  if grep -q 'Edit_Profile' <<<"$snapshot"; then tap 251 610 4; continue; fi
  if grep -qE '(Idle|Online|Do_Not_Disturb|Invisible) 165 623' <<<"$snapshot"; then tap 165 623 3; tap 251 610 4; continue; fi
  tap 28 48 2
done
labels
exit 1
