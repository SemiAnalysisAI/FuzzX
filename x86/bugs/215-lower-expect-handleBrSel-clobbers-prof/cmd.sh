#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes=lower-expect -S repro.ll | grep -E "define|br |ret|prof"
echo "(output !prof !0 reflects expect-style weights replacing measured ones)"
"$OPT" -passes=lower-expect -S repro.ll | grep "^!"
