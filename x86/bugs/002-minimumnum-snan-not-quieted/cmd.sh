#!/usr/bin/env bash
set -euo pipefail
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
"$LLC" -O2 -mtriple=x86_64-unknown-linux-gnu -filetype=obj repro.ll -o repro.o
cc -O0 runner.c repro.o -o runner
./runner; echo "exit=$?"
