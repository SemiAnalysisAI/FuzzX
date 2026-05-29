#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== nnan/ninf fold returns NaN/Inf instead of poison (LangRef violation) ====="
"$OPT" -passes=instcombine -S repro.ll | grep -E "define|ret"
