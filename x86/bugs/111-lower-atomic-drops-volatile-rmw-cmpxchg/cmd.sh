#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== after lower-atomic — volatile dropped from both load and store ====="
"$OPT" -passes=lower-atomic -S repro.ll | grep -E "define|load|store|cmpxchg|atomicrmw|ret"
