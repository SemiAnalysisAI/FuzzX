#!/usr/bin/env bash
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
echo "===== store atomic volatile i128 syncscope(\"singlethread\") → cmpxchg loop WITHOUT volatile or syncscope ====="
"$LLC" -mtriple=x86_64-linux-gnu -mattr=+cx16 -stop-after=atomic-expand repro.ll -o - 2>&1 | grep -E "define|cmpxchg|load|store|atomicrmw|ret"
