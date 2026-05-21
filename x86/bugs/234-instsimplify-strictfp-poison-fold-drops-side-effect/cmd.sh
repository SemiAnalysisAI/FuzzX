#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes=instsimplify -S repro.ll | grep -E "define|ret|poison|constrained|call"
