#!/usr/bin/env bash
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
"$LLC" -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=machine-cse repro.ll -o - 2>&1 | grep -E "MOV32rm|range|MMO" | head
