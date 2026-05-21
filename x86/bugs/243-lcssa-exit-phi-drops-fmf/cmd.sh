#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes=lcssa -S repro.ll | grep -E "define|fmul|phi|ret|nnan"
