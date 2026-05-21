#!/usr/bin/env bash
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "== with instcombine alone (gather intact, range fold works) =="
"$OPT" -passes=instcombine -mtriple=x86_64-- -S repro.ll | grep -E "define|gather|icmp|ret"
echo "== with scalarize-masked-mem-intrin first (range lost, fold blocked) =="
"$OPT" -passes='scalarize-masked-mem-intrin,instcombine' -mtriple=x86_64-- -S repro.ll | grep -E "define|load|icmp|ret" | head
