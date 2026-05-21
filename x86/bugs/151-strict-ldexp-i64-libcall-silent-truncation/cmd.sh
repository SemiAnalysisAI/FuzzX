#!/usr/bin/env bash
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
echo "===== strict ldexp f64.i64 → bare ldexp@PLT, high 32 of %rdi dropped (strict variant of #011) ====="
"$LLC" -mtriple=x86_64-linux-gnu repro.ll -o - | sed -n '/^strict_ldexp_i64:/,/Lfunc_end/p'
