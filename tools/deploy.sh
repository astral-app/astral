#!/bin/bash
set -euo pipefail
device="${DEVICE:?set DEVICE to the test device UDID}"
pilot="${JB_PILOT:-$HOME/go/bin/jb-p1lot}"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/packages/loader-ios"
export PATH=/opt/homebrew/opt/make/libexec/gnubin:/opt/homebrew/bin:$PATH THEOS="${THEOS:-$HOME/theos}"
make package FINALPACKAGE=1 >"${TMPDIR:-/tmp}/astral-build.log" 2>&1 || {
  tail -30 "${TMPDIR:-/tmp}/astral-build.log"
  exit 1
}
package=""
for candidate in packages/*.deb; do
  [ -e "$candidate" ] && package="$candidate"
done
"$pilot" file_transfer --device "$device" --direction host_to_device --source "$PWD/$package" --destination /var/mobile/Documents/astral.deb >/dev/null
"$pilot" shell_exec --device "$device" --command "dpkg -i /var/mobile/Documents/astral.deb >/dev/null 2>&1; killall -9 Discord 2>/dev/null; sleep 1; /var/jb/usr/bin/uiopen --bundleid com.hammerandchisel.discord; echo installed $(basename "$package")"
