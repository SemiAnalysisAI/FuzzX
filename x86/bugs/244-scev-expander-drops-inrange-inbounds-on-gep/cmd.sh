#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes=indvars -S repro.ll | grep -E "define|getelementptr|inbounds|inrange|scevgep"
