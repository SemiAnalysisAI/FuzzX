#!/usr/bin/env bash
LLC=$HOME/code/llvm3/build/bin/llc
"$LLC" -O2 -mtriple=x86_64-linux-gnu repro.ll -o /dev/null
# also reproduces with the vector form (e.g. constrained.fadd.v2bf16)
