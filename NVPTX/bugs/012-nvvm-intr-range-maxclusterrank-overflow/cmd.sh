#!/usr/bin/env bash
set -uo pipefail
TOOL="${TOOL:-/Users/justinlebar/code/llvm2/build/bin/opt}"
cd "$(dirname "$0")"
echo "===== opt -mtriple=nvptx64 -mcpu=sm_90 -passes=nvvm-intr-range -S -o - repro.ll ====="
"$TOOL" -mtriple=nvptx64 -mcpu=sm_90 -passes=nvvm-intr-range -S -o - repro.ll
echo "exit=$?"
