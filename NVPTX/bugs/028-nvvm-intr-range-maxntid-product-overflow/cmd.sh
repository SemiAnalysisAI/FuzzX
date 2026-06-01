#!/usr/bin/env bash
set -uo pipefail
TOOL="${TOOL:-/Users/justinlebar/code/llvm2/build/bin/opt}"
cd "$(dirname "$0")"
echo "===== opt -S -mtriple=nvptx64-nvidia-cuda -passes=nvvm-intr-range -o - repro.ll ====="
"$TOOL" -S -mtriple=nvptx64-nvidia-cuda -passes=nvvm-intr-range -o - repro.ll
echo "exit=$?"
