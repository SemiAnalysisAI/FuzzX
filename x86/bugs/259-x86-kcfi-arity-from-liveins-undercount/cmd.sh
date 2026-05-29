#!/usr/bin/env bash
LLC=$HOME/code/llvm3/build/bin/llc
# ABI arity of void(i32,ptr) is 2, but the prefix encodes arity 1 (%ecx) because
# RDI (event) is dead and dropped from liveins; ctx lives in RSI (2nd reg).
"$LLC" -O2 -mtriple=x86_64-unknown-linux-gnu repro.ll -o - | grep -E 'movl.*199571451|\(%rsi\)'
