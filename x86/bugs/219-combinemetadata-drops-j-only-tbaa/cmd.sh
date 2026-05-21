#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes='early-cse<memssa>' -S repro.ll | grep -E "define|load|add|ret|tbaa"
