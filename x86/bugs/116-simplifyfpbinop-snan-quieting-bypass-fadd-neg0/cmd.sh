#!/usr/bin/env bash
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
"$LLC" -O2 -mtriple=x86_64-linux-gnu repro.ll -o - | sed -n '/^fadd_neg0\|^fsub_pos0/,/Lfunc_end/p'
