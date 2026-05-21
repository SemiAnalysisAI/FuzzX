#!/usr/bin/env bash
set -euo pipefail
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
echo "===== asm (expect: jmp __x86_return_thunk; observed: bare retl \$4) ====="
"$LLC" -O2 repro.ll -o - | sed -n '/^foo:/,/Lfunc_end/p'
