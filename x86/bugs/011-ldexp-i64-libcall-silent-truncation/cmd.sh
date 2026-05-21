#!/usr/bin/env bash
set -euo pipefail
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
echo "===== asm: bare tail call, no truncation ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu repro.ll -o - | sed -n '/^t:/,/Lfunc_end/p'
echo "===== runtime: pass an i64 with non-zero high 32 bits ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu -filetype=obj repro.ll -o repro.o
cc -O0 runner.c repro.o -o runner -lm
./runner; echo "exit=$?"
