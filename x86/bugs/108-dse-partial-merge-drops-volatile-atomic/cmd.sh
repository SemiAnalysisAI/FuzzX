#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== before DSE ====="
"$OPT" -S repro.ll | grep -E "store|define"
echo "===== after DSE — volatile and atomic stores GONE, merged into plain store ====="
"$OPT" -passes=dse -S repro.ll | grep -E "store|define"
