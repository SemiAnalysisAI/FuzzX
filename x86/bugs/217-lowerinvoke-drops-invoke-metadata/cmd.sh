#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes=lower-invoke -S repro.ll | grep -E "define|call|invoke|ret|!prof|!annotation|!range"
