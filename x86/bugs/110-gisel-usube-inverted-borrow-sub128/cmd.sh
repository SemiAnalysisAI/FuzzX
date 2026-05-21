#!/usr/bin/env bash
set -euo pipefail
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
echo "===== GISel asm (note cmpb \$1, %sil which inverts CF) ====="
"$LLC" -O0 -mtriple=x86_64-linux-gnu -global-isel repro.ll -o - | sed -n '/^sub_i128:/,/Lfunc_end/p'
echo "===== Runtime ====="
"$LLC" -O0 -mtriple=x86_64-linux-gnu -global-isel -filetype=obj repro.ll -o repro.o
cc -O0 runner.c repro.o -o runner
./runner; echo "exit=$?"
