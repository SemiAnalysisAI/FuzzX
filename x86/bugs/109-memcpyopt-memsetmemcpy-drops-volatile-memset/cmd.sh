#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== before MemCpyOpt ====="
"$OPT" -S repro.ll | grep -E "memset|memcpy|alloca|use"
echo "===== after MemCpyOpt — volatile memset replaced/dropped ====="
"$OPT" -passes=memcpyopt -S repro.ll | grep -E "memset|memcpy|alloca|use"
