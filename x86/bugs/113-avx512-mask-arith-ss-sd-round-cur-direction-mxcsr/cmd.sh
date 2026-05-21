#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== Intrinsic with CUR_DIRECTION (=4) rounding folded to plain fadd, losing MXCSR semantics ====="
"$OPT" -passes=instcombine -S repro.ll | grep -E "define|call|extract|insert|fadd|ret"
