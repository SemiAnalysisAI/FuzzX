#!/usr/bin/env bash
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
echo "===== llc -O2: both fmul X,1.0 and fdiv X,1.0 emit just retq (no mulss/divss) ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu repro.ll -o - | sed -n '/^fmul_one\|^fdiv_one/,/Lfunc_end/p'
