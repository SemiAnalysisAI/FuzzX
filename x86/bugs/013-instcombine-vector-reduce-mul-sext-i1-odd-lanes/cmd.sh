#!/usr/bin/env bash
set -euo pipefail
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc

echo "===== IR after opt -passes=instcombine (note: result type range is i8 [0,2)) ====="
"$OPT" -passes=instcombine -S repro.ll | sed -n '/define i8 @f/,/^}/p'

echo "===== final asm after O2 — returns 0/1, not -1 ====="
"$OPT" -passes='default<O2>' -S repro.ll | "$LLC" -mtriple=x86_64-linux-gnu -O2 -o - \
  | sed -n '/^f:/,/Lfunc_end/p'

echo "===== runtime ====="
"$OPT" -passes='default<O2>' -S repro.ll \
  | "$LLC" -mtriple=x86_64-linux-gnu -O2 -filetype=obj -o repro.o
cc -O0 runner.c repro.o -o runner
./runner; echo "exit=$?"
