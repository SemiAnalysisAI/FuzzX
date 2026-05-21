#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== atomic unordered <4 x i32> load → N plain non-atomic i32 loads (tearable) ====="
"$OPT" -passes='vector-combine' -S repro.ll | grep -E "define|load|extract|add|ret"
