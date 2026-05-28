#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== After newgvn: deopt bundle dropped via CSE ====="
"$OPT" -passes=newgvn -S repro.ll | grep -E "define|call|add|ret"
