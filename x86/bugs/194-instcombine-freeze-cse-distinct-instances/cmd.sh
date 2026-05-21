#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== InstCombine alone CSEs two distinct freeze of poison-able operand ====="
"$OPT" -passes=instcombine -S repro.ll | grep -E "define|freeze|sub|ret"
