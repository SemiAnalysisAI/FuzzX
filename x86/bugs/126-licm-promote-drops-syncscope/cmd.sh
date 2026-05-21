#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== Before LICM: load/store have syncscope(\"singlethread\") ====="
"$OPT" -S repro.ll | grep -E "load|store"
echo "===== After LICM promote: syncscope(\"singlethread\") DROPPED ====="
"$OPT" -passes='loop-mssa(licm)' -S repro.ll | grep -E "load|store|define|ret"
