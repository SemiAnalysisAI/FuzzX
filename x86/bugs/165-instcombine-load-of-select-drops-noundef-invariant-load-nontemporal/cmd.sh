#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== load-of-select fold: only PoisonGeneratingIDs copied; !noundef/!invariant.load/!nontemporal/AA dropped ====="
"$OPT" -passes=instcombine -S repro.ll | grep -E "define|load|select|noundef|invariant|nontemporal|ret"
