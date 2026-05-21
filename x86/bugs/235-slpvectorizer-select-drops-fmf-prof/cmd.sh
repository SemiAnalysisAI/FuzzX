#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes=slp-vectorizer -S repro.ll | grep -E "define|select|nnan|store|ret" | head
