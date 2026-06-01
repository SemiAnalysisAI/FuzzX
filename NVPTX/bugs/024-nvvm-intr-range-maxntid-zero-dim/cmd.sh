#!/usr/bin/env bash
set -uo pipefail
# NOTE: this is `opt` (the pass runs in the optimization pipeline); plain `llc` codegen does NOT run nvvm-intr-range.
TOOL="${TOOL:-/Users/justinlebar/code/llvm2/build/bin/opt}"
cd "$(dirname "$0")"
echo "===== opt -S -mtriple=nvptx64-nvidia-cuda -passes=nvvm-intr-range repro.ll ====="
"$TOOL" -S -mtriple=nvptx64-nvidia-cuda -passes=nvvm-intr-range repro.ll -o -
echo "exit=$?"
