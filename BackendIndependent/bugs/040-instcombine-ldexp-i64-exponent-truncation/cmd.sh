#!/usr/bin/env bash
set -euo pipefail

OPT=${OPT:-/Users/justinlebar/code/llvm2/build/bin/opt}
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

"$OPT" -passes=verify -disable-output repro.ll
"$OPT" -passes=instcombine -S repro.ll >"$tmp"

grep -F "%r = fmul double %x, f0x4210000000000000" "$tmp"
