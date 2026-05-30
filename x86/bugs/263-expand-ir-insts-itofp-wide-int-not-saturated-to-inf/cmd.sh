#!/usr/bin/env bash
# Use the FuzzX build, or any opt/llc predating PR #200291.
OPT=${OPT:-../amdgpu/build/llvm-fuzzer/bin/opt}
LLC=${LLC:-../amdgpu/build/llvm-fuzzer/bin/llc}

# 1) Show the expansion has no inf-saturation select (the fix adds `select i1 ..., float 0x7FF0000000000000, ...`):
"$OPT" -S -mtriple=x86_64-- --expand-ir-insts < repro.ll | grep -i "0x7FF0000000000000\|select i1 .*float" || echo "NO inf saturation -> buggy"

# 2) Self-contained constant: 2^200 -> float must be +Inf (0x7F800000).
#    Unfixed codegen returns 0xA4000000 (~ -2.66e-17).
"$LLC" -O2 -mtriple=x86_64-unknown-linux-gnu repro.ll -o -
