#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== after simplifycfg — atomic unordered stores merged into plain store ====="
"$OPT" -passes='simplifycfg<>' -S repro.ll | grep -E "define|store|select|or|br"
