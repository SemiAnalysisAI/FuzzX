#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== Two seq_cst atomic loads in mutually-exclusive branches → one hoisted unconditional load ====="
"$OPT" -passes='simplifycfg<hoist-common-insts>' -S repro.ll | grep -E "define|load|add|select|phi|br|ret"
