#!/usr/bin/env bash
LLC=$HOME/code/llvm3/build/bin/llc
"$LLC" -O2 -mtriple=x86_64-linux-gnu repro.ll -o /dev/null
# also: llvm.minimum.f128, llvm.vector.reduce.fmaximum/fminimum.vNf128
