#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== Two atomic unordered loads w/ different syncscope, CSE'd to one (narrower syncscope kept) ====="
"$OPT" -passes=early-cse -S repro.ll | grep -E "define|load|add|ret"
