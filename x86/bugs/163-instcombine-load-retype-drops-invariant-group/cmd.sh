#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== !invariant.group dropped on retyped load (copyMetadataForLoad missing case) ====="
"$OPT" -passes=instcombine -S repro.ll | grep -E "define|load|invariant|ret"
