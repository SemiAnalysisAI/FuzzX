#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== Two atomic unordered stores w/ different syncscope, earlier dropped ====="
"$OPT" -passes=early-cse -S repro.ll | grep -E "define|store|ret"
