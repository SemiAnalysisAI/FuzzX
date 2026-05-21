#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes=gvn -S repro.ll | grep -E "define|add|extractvalue|use|ret"
