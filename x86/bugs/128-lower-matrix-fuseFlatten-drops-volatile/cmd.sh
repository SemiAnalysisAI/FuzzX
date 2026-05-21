#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== matrix.column.major.load with isVolatile=true rewritten as non-volatile load ====="
"$OPT" -passes=lower-matrix-intrinsics -S repro.ll | grep -E "define|load|store"
