#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes=correlated-propagation -S repro.ll | grep -E "define|select|add|ret|range"
