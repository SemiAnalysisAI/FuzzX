#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
"$OPT" -passes='scalarize-masked-mem-intrin' -mtriple=x86_64-- -S repro.ll | grep -E "define|load|nontemporal|br |ret" | head
