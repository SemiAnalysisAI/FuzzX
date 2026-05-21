#!/usr/bin/env bash
set -euo pipefail
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
echo "===== MIR going into X86FixupInstTuning (PSLLWri) ====="
"$LLC" -mtriple=x86_64-linux-gnu -mattr=+sse2 -stop-before=x86-fixup-inst-tuning repro.ll -o - | grep -E "name:|PSLLWri|PADDWrr"
echo "===== MIR after X86FixupInstTuning (PADDWrr — mutation confirmed) ====="
"$LLC" -mtriple=x86_64-linux-gnu -mattr=+sse2 -stop-after=x86-fixup-inst-tuning repro.ll -o - | grep -E "name:|PSLLWri|PADDWrr"
echo
echo "The pass mutated PSLLWri -> PADDWrr but ProcessShiftLeftToAdd returns false"
echo "(see X86FixupInstTuning.cpp line ~307), so Changed stays false and the pass"
echo "lies about preservation. NumInstChanges stat stays 0."
