#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== SROA memcpy split: per-load !tbaa+!nontemporal dropped, memcpy's tbaa substituted ====="
"$OPT" -passes=sroa -S repro.ll | grep -E "load|tbaa|nontemporal|define"
