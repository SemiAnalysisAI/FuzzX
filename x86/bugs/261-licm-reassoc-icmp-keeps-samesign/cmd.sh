#!/usr/bin/env bash
OPT=$HOME/code/llvm3/build/bin/opt
"$OPT" -passes='loop-mssa(licm)' -S repro.ll | grep icmp
# BUGGY: 'icmp samesign slt i32 %iv, 95' keeps samesign; FIXED: 'icmp slt i32 %iv, 95'
