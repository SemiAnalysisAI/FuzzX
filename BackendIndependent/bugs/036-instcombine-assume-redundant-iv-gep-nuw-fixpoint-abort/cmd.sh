#!/usr/bin/env bash
set -euo pipefail

OPT=${OPT:-/Users/justinlebar/code/llvm2/build/bin/opt}
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

if "$OPT" -passes='require<domtree>,instcombine,require<domtree>' -S repro.ll >"$tmp" 2>&1; then
  echo "expected instcombine fixpoint abort, but command succeeded"
  cat "$tmp"
  exit 1
fi

grep -F "did not reach a fixpoint" "$tmp"
