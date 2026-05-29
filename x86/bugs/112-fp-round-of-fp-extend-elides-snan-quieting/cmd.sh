#!/usr/bin/env bash
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
echo "===== llc -O2: the fpext/fptrunc round-trip is elided to a no-op ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu repro.ll -o - | sed -n '/^snan_round_trip:/,/Lfunc_end/p'
