#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes=instcombine -S repro.ll | grep -E "define|load|ret|!nontemporal"
