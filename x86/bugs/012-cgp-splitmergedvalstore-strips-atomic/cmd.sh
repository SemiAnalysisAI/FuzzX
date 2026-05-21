#!/usr/bin/env bash
set -euo pipefail
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc

echo "===== IR after CGP — atomic i64 store split into two NON-atomic i32 stores ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu -stop-after=codegenprepare repro.ll -o - 2>&1 \
  | sed -n '/define void @atom_fp/,/^  }/p'

echo
echo "===== compare: non-atomic version (same output, expected) ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu -stop-after=codegenprepare repro.ll -o - 2>&1 \
  | sed -n '/define void @nonatom_fp/,/^  }/p'

echo
echo "===== final asm — the 'atomic' store is two ordinary movs ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu repro.ll -o - | sed -n '/^atom_fp:/,/Lfunc_end/p'
