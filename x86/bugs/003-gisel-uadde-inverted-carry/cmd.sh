#!/usr/bin/env bash
set -euo pipefail
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc

echo "===== asm with -global-isel (the buggy path) ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu -global-isel repro.ll -o - | sed -n '/^add128:/,/Lfunc_end/p'

echo "===== asm without -global-isel (correct, for comparison) ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu repro.ll -o - | sed -n '/^add128:/,/Lfunc_end/p'

echo "===== run the GISel-compiled object ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu -global-isel -filetype=obj repro.ll -o repro.o
cc -O0 runner.c repro.o -o runner
./runner; echo "exit=$?"
