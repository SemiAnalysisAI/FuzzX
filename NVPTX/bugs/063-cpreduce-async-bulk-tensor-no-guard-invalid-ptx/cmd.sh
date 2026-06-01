#!/usr/bin/env bash
set -uo pipefail
TOOL="${TOOL:-/Users/justinlebar/code/llvm2/build/bin/llc}"
cd "$(dirname "$0")"
echo "===== llc -mtriple=nvptx64 -mcpu=sm_60 -mattr=+ptx60 -o - repro.ll ====="
"$TOOL" -mtriple=nvptx64 -mcpu=sm_60 -mattr=+ptx60 -o - repro.ll
echo "exit=$?"
