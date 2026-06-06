#!/usr/bin/env bash
set -euo pipefail

OPT=${OPT:-/Users/justinlebar/code/llvm2/build/bin/opt}
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

"$OPT" -passes=verify -disable-output repro.ll

if "$OPT" -passes=instcombine -S repro.ll >"$tmp" 2>&1; then
  echo "expected scalarizePHI crash/assert, but command succeeded"
  cat "$tmp"
  exit 1
fi

grep -E "scalarizePHI|isKnownSentinel|Assertion failed" "$tmp"
