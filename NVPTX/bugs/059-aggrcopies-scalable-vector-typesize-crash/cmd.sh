#!/usr/bin/env bash
set -uo pipefail
TOOL="${TOOL:-/Users/justinlebar/code/llvm2/build/bin/llc}"
cd "$(dirname "$0")"
echo "===== llc -O0 -mtriple=nvptx64 -o - repro.ll ====="
"$TOOL" -O0 -mtriple=nvptx64 -o - repro.ll
echo "exit=$?"
