#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== gvn-sink: per-path deopt operand merged via PHI (deopt frame corrupted) ====="
"$OPT" -passes=gvn-sink -S repro.ll | grep -E "define|call|phi|deopt|br|ret"
