#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== SimplifyCFG mergeConditionalStores drops !nontemporal when only one carries it ====="
"$OPT" -passes='simplifycfg<>' -S repro.ll | grep -E "define|store|nontemporal"
