#!/usr/bin/env bash
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
echo "===== llc -O2 emits xorps sign-mask (no divsd) ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu repro.ll -o - | sed -n '/^fdiv_neg1:/,/Lfunc_end/p'
