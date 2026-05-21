#!/usr/bin/env bash
set -euo pipefail
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc

echo "===== buggy lowering (-mattr=+avx512f, no +avx512vl) ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu -mattr=+avx512f -filetype=asm repro.ll -o - | sed -n '/^ldexp_v4f32:/,/Lfunc_end/p'

echo "===== correct lowering (+avx512f,+avx512vl) ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu -mattr=+avx512f,+avx512vl -filetype=asm repro.ll -o - | sed -n '/^ldexp_v4f32:/,/Lfunc_end/p'

echo "===== build & run the AVX512F-only object (host must support AVX-512) ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu -mattr=+avx512f -filetype=obj repro.ll -o repro.o
cc -O0 runner.c repro.o -o runner
./runner; echo "exit=$?"
