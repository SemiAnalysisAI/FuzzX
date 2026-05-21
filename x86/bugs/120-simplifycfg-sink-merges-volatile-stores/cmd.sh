#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== Two volatile stores in mutually-exclusive branches → one sunk volatile store with select-fed value ====="
"$OPT" -passes='simplifycfg<sink-common-insts>' -S repro.ll | grep -E "define|store|select|br|ret"
