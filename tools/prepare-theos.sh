#!/bin/bash
set -euo pipefail
theos="${THEOS:?set THEOS to the Theos checkout}"
theos_head="$(git ls-remote https://github.com/theos/theos HEAD | awk '{ print $1 }')"
sdks_head="$(git ls-remote https://github.com/theos/sdks HEAD | awk '{ print $1 }')"
brew install ldid make
if [ ! -d "$theos" ]; then
  git clone --recursive https://github.com/theos/theos "$theos"
elif [ "$(git -C "$theos" rev-parse HEAD)" != "$theos_head" ]; then
  git -C "$theos" pull --ff-only --recurse-submodules
  git -C "$theos" submodule update --init --recursive
fi
if [ "$(cat "$theos/sdks/.revision" 2>/dev/null)" != "$sdks_head" ]; then
  sdks="$(mktemp -d)"
  git clone --depth 1 https://github.com/theos/sdks "$sdks"
  find "$theos/sdks" -mindepth 1 -maxdepth 1 -name '*.sdk' -exec rm -rf {} +
  mv "$sdks"/*.sdk "$theos/sdks"
  git -C "$sdks" rev-parse HEAD >"$theos/sdks/.revision"
  rm -rf "$sdks"
fi
