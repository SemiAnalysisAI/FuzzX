#!/usr/bin/env bash
set -euo pipefail
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
echo "===== llc asm — note the catch funclet entry has NO endbr64 ====="
"$LLC" -O2 repro.ll -o - | grep -E "endbr|catch|cleanup|^f:|\.LBB|\.seh_|\.text"
