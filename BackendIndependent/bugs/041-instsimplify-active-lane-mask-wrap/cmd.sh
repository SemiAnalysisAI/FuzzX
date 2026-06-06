#!/usr/bin/env bash
set -euo pipefail

OPT=${OPT:-/Users/justinlebar/code/llvm2/build/bin/opt}
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

"$OPT" -passes=verify -disable-output repro.ll
"$OPT" -passes=instsimplify -S repro.ll >"$tmp"

grep -F "ret <4 x i1> <i1 false, i1 false, i1 true, i1 true>" "$tmp"
