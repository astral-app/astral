#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
stylua plugins
RUST_LOG=warn taplo format
git ls-files -z -- 'packages/*.c' 'packages/*.h' 'packages/*.m' 'packages/*.mm' 'packages/*.cpp' | xargs -0 clang-format -i
git ls-files -z -- 'tools/*.sh' | xargs -0 shfmt -w -i 2 -ci
