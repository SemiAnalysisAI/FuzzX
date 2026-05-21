#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes=function-attrs -S repro.ll | grep -E "Function Attrs|define"
