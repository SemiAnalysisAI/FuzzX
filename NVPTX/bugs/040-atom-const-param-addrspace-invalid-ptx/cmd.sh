#!/usr/bin/env bash
set -uo pipefail
TOOL="${TOOL:-/Users/justinlebar/code/llvm2/build/bin/llc}"
cd "$(dirname "$0")"
echo "===== llc -mtriple=nvptx64 -mcpu=sm_50 -mattr=+ptx50 -o - repro.ll ====="
"$TOOL" -mtriple=nvptx64 -mcpu=sm_50 -mattr=+ptx50 -o - repro.ll
echo "exit=$?"
