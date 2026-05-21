#!/usr/bin/env bash
set -euo pipefail
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
echo "===== llc with -mattr=+lvi-cfi (expect: pop/lfence/jmp; observed: bare retq \$8) ====="
"$LLC" -O2 -mtriple=x86_64-linux-gnu -mattr=+lvi-cfi repro.ll -o - | sed -n '/^f:/,/Lfunc_end/p'
