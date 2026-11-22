#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cyan_repository="https://github.com/asdfzxcvbn/pyzule-rw"
cyan_commit="740d3716dcd98c20c000f12cdb88f1f0b2a533a4"
pillow_version="11.3.0"

usage() { echo "usage: build-ipa.sh --ipa <decrypted Discord.ipa> [--deb <astral.deb>] [--output <astral.ipa>]" >&2; exit 2; }
ipa="" deb="" output=""
while [ $# -gt 0 ]; do
  case "$1" in
    --ipa) ipa="$2"; shift 2 ;;
    --deb) deb="$2"; shift 2 ;;
    --output) output="$2"; shift 2 ;;
    *) usage ;;
  esac
done
[ -n "$ipa" ] || usage
[ -f "$ipa" ] || { echo "IPA not found: $ipa" >&2; exit 1; }
if [ -z "$deb" ]; then
  deb="$(ls -t "$root"/packages/loader-ios/packages/*.deb 2>/dev/null | head -1 || true)"
  [ -n "$deb" ] || { echo "no astral .deb; build packages/loader-ios first" >&2; exit 1; }
fi
output="${output:-$root/astral.ipa}"

cache="$root/.tools-cache/cyan-$cyan_commit"
cyan="$cache/venv/bin/cyan"
if [ ! -x "$cyan" ]; then
  mkdir -p "$cache"
  python3 -m venv "$cache/venv"
  "$cache/venv/bin/python3" -m pip install --disable-pip-version-check --quiet "$cyan_repository/archive/$cyan_commit.zip" "Pillow==$pillow_version"
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
unzip -q -o "$ipa" 'Payload/*.app/Info.plist' -d "$work"
info="$(ls "$work"/Payload/*.app/Info.plist)"
executable="$(plutil -extract CFBundleExecutable raw "$info")"
unzip -q -o "$ipa" "Payload/*.app/$executable" -d "$work"
if otool -L "$work"/Payload/*.app/"$executable" | grep -q "Astral.dylib"; then
  echo "the IPA already loads Astral.dylib; use a clean Discord IPA" >&2
  exit 1
fi

"$root/tools/stage-resources.sh" "$work/Astral.bundle"
"$cyan" -duwsgq -i "$ipa" -o "$work/astral.ipa" -f "$deb" "$work/Astral.bundle"
mv "$work/astral.ipa" "$output"
echo "created $output"
