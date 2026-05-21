#!/usr/bin/env bash
LLC=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc
"$LLC" -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=finalize-isel repro.ll -o - | grep -E "MOV|store|nontemporal|tbaa"
