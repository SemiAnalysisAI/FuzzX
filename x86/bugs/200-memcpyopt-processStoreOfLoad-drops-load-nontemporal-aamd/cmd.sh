#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes=memcpyopt -S repro.ll | grep -E "define|memcpy|load|store|use|nontemporal"
