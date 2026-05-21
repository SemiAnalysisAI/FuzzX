#!/usr/bin/env bash
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
echo "===== frexp.f64.i64 — 8B slot, libcall writes 4B (int), load reads 8B → high 4B = stale %rax (info leak) ====="
"$LLC" -mtriple=x86_64-linux-gnu repro.ll -o - | sed -n '/^frexp_i64:/,/Lfunc_end/p'
