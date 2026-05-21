#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes=jump-threading -S repro.ll | grep -E "define|br |select|freeze|ret"
