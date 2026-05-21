#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== fence hoisted out of loop (acquire, seq_cst); collapsing N fences to 1 ====="
"$OPT" -passes='loop-mssa(licm)' -S repro.ll | grep -E "define|fence|phi|br|ret"
