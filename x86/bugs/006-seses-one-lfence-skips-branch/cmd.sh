#!/usr/bin/env bash
set -euo pipefail
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
echo "===== with -x86-seses-one-lfence-per-bb (buggy: branch lfence missing) ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu \
  -x86-seses-enable-without-lvi-cfi -x86-seses-one-lfence-per-bb \
  repro.ll -o - | sed -n '/^f:/,/Lfunc_end/p'
echo "===== without -x86-seses-one-lfence-per-bb (correct: lfence before branch too) ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu -x86-seses-enable-without-lvi-cfi \
  repro.ll -o - | sed -n '/^f:/,/Lfunc_end/p'
