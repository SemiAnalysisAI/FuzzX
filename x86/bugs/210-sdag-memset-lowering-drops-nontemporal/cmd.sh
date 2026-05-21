#!/usr/bin/env bash
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
"$LLC" -O2 -mtriple=x86_64-unknown-linux-gnu -mattr=+avx2 -stop-after=finalize-isel repro.ll -o - 2>&1 | grep -E "VMOV|MMO|non-temporal" | head
