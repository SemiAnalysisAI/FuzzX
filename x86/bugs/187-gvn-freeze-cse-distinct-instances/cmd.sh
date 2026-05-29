#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== Standard GVN (default O2) CSEs two freezes of same operand → 'ret i32 0' ====="
"$OPT" -passes=gvn -S repro.ll | grep -E "define|freeze|sub|ret"
