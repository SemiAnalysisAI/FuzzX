#!/usr/bin/env bash
LLC=$HOME/code/llvm3/build/bin/llc
"$LLC" -O2 -mtriple=x86_64-linux-gnu repro.ll -o /dev/null
# fcmps (signaling) variant crashes identically
