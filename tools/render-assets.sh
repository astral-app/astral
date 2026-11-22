#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
render() {
  local name="$1" points="$2"
  for scale in 1 2 3; do
    local suffix=""
    [ "$scale" -gt 1 ] && suffix="@${scale}x"
    magick -background none -density 900 "$root/assets/$name.svg" -resize "$((points * scale))x$((points * scale))" "PNG32:$root/assets/rendered/$name$suffix.png"
  done
}
render galaxy 24
render plug 22
