#!/usr/bin/env bash
set -euo pipefail

OPT=${OPT:-/Users/justinlebar/code/llvm2/build/bin/opt}
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

"$OPT" -passes=instcombine -S repro.ll -o "$tmp"

grep -F "ret i1 false" "$tmp"
