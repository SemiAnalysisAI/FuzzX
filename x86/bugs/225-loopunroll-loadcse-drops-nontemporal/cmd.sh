#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes=loop-unroll -unroll-allow-partial -unroll-count=2 -S repro.ll | grep -E "define|load|add|ret|!nontemporal" | head
