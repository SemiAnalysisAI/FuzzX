#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== memcpy.element.unordered.atomic elt=1, len=4 collapsed to single i32 atomic store ====="
"$OPT" -passes=instcombine -S repro.ll | grep -E "define|memcpy|atomic|load|store|ret"
