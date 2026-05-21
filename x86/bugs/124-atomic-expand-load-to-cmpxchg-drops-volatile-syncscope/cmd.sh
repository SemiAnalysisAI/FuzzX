#!/usr/bin/env bash
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
echo "===== load atomic volatile i128 syncscope(\"singlethread\") seq_cst → cmpxchg WITHOUT volatile or syncscope ====="
"$LLC" -mtriple=x86_64-linux-gnu -mattr=+cx16 -stop-after=atomic-expand repro.ll -o - 2>&1 | grep -E "define|cmpxchg|load|extract|ret"
