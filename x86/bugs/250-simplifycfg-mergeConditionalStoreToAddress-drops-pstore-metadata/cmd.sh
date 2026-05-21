#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes=simplifycfg -S repro.ll | grep -E "define|store|select|nontemporal|tbaa|ret"
