#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== after sroa — atomic seq_cst is dropped from both load and store ====="
"$OPT" -passes=sroa -S repro.ll | grep -E "define|load|store|atomic|ret"
