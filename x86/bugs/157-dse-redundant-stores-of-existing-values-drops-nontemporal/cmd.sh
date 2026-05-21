#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== DSE keeps later non-nontemporal store, deletes earlier nontemporal — !nontemporal dropped ====="
"$OPT" -passes=dse -S repro.ll | grep -E "define|store|ret"
