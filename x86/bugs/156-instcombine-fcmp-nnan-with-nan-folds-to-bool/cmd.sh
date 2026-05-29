#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== fcmp nnan with NaN constant folds to false/true instead of poison ====="
"$OPT" -passes=instcombine -S repro.ll | grep -E "define|ret"
