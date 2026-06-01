#!/usr/bin/env bash
set -uo pipefail
TOOL="${TOOL:-/Users/justinlebar/code/llvm2/build/bin/llc}"
cd "$(dirname "$0")"
echo "===== llc -mcpu=sm_20 -o - repro.ll ====="
"$TOOL" -mcpu=sm_20 -o - repro.ll
echo "exit=$?"
