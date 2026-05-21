#!/usr/bin/env bash
set +e
OPT=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt
echo "===== Repro: opt -passes=codegenprepare on a trivial function ====="
"$OPT" -passes=codegenprepare -mtriple=x86_64-linux-gnu -S repro.ll > /dev/null 2>&1
ec=$?
echo "exit status: $ec (139 = SIGSEGV)"
[ $ec -eq 139 ] && echo "REPRODUCED — crash" || echo "did not reproduce"
