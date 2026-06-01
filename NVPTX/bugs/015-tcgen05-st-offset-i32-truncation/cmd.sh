#!/usr/bin/env bash
set -uo pipefail
TOOL="${TOOL:-/Users/justinlebar/code/llvm2/build/bin/llc}"
cd "$(dirname "$0")"
echo "===== llc -march=nvptx64 -mcpu=sm_100a -mattr=+ptx86 -o - repro.ll ====="
"$TOOL" -march=nvptx64 -mcpu=sm_100a -mattr=+ptx86 -o - repro.ll
echo "exit=$?"
