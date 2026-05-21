#!/usr/bin/env bash
set -euo pipefail
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
"$LLC" -O2 -mtriple=x86_64-linux-gnu -stop-after=finalize-isel repro.ll -o - | grep -E "FLDENV|ldmxcsr|MOStore|MOLoad" || echo "(scheduler-dependent; this dumps MIR for inspection — see NOTES.md)"
