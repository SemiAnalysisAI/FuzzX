#!/usr/bin/env bash
set -euo pipefail

OPT=${OPT:-/Users/justinlebar/code/llvm2/build/bin/opt}
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

"$OPT" -passes=verify -disable-output repro.ll
"$OPT" -passes=instcombine -S repro.ll >"$tmp"

grep -F "%v = phi i32 [ -1, %neg ], [ -1, %pos ], [ %y, %unk ]" "$tmp"
grep -F "%cmp1 = icmp samesign ne i32 %v, 0" "$tmp"
