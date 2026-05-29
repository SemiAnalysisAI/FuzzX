#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== LICM hoists conditional load to unconditional preheader (UB injection if p not deref) ====="
"$OPT" -passes='loop-mssa(licm<allowspeculation>)' -S repro.ll | grep -E "define|load|store|phi|br"
