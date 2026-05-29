#!/usr/bin/env bash
LLC=$HOME/code/llvm3/build/bin/llc
echo "== +avx512f,+egpr (BWI OFF) -> wrong: kmovq (KMOVQkk_EVEX, requires BWI) =="
"$LLC" -O2 -mtriple=x86_64-unknown-linux-gnu -mattr=+avx512f,+egpr repro.ll -o - | grep kmov
echo "== control +avx512f -> correct: kmovw =="
"$LLC" -O2 -mtriple=x86_64-unknown-linux-gnu -mattr=+avx512f repro.ll -o - | grep kmov
