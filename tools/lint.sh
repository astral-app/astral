#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
stylua --check plugins
RUST_LOG=warn taplo lint
RUST_LOG=warn taplo format --check
luacheck --quiet plugins
git ls-files -z -- 'packages/*.c' 'packages/*.h' 'packages/*.m' 'packages/*.mm' 'packages/*.cpp' | xargs -0 clang-format --dry-run -Werror
git ls-files -z -- 'tools/*.sh' 'tools/astral-dev' | xargs -0 shfmt -d -i 2 -ci
git ls-files -z -- 'tools/*.sh' 'tools/astral-dev' | xargs -0 shellcheck
