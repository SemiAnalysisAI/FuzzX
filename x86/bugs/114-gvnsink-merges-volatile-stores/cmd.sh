#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== before gvn-sink: two volatile stores ====="
"$OPT" -S repro.ll | grep -E "define|store|phi"
echo "===== after gvn-sink: merged into one sunk store with phi-fed value ====="
"$OPT" -passes=gvn-sink -S repro.ll | grep -E "define|store|phi"
