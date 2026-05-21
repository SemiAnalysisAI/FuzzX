#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes=lower-expect -S repro.ll | grep -E "switch|prof|i32 [0-9]+, label"
"$OPT" -passes=lower-expect -S repro.ll | grep "^!"
