#!/usr/bin/env bash
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
"$LLC" -O2 -mtriple=x86_64-unknown-linux-gnu -print-after=branch-folder repro.ll -o - 2>&1 | grep -E "ADD32|nuw|add" | head
