#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== InstCombine folds chained ldexp(INT_MAX, INT_MAX) to fmul x, 0.25 ====="
"$OPT" -passes=instcombine -S repro.ll | grep -E "define|ldexp|fmul|ret"
