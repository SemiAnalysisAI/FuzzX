#!/usr/bin/env bash
OPT=$HOME/code/llvm3/build/bin/opt
"$OPT" -passes=mergeicmps -mtriple=x86_64-unknown-unknown -S repro.ll | grep -E 'memcmp|icmp eq i17'
# BUGGY: merges two icmp eq i17 into a byte memcmp; FIXED: left as two icmp eq i17
