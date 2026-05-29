#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== EarlyCSE (default O2) CSEs two freezes ====="
"$OPT" -passes='early-cse<memssa>' -S repro.ll | grep -E "define|freeze|sub|ret"
