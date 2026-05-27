#!/usr/bin/env bash
# Build the in-process SPIR-V backend crash libFuzzer target.
# Mirrors amdgpu/scripts/build_directed_fuzzer.sh, with HIP/LLD removed.
#
# Required unless using the default instrumented build path:
#   LLVM_DIR=/path/to/llvm-build/lib/cmake/llvm
#
# Optional:
#   FUZZER_SANITIZERS=fuzzer,address
#   CMAKE_BUILD_TYPE=Release

set -euo pipefail

cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.."
ROOT="$(pwd)"

LLVM_BUILD_DIR="${LLVM_BUILD_DIR:-$ROOT/build/llvm-fuzzer}"
LLVM_DIR="${LLVM_DIR:-$LLVM_BUILD_DIR/lib/cmake/llvm}"
FUZZER_BUILD_DIR="${FUZZER_BUILD_DIR:-$ROOT/build/fuzzer}"
FUZZER_SANITIZERS="${FUZZER_SANITIZERS:-fuzzer}"
CMAKE_BUILD_TYPE="${CMAKE_BUILD_TYPE:-Release}"

if [[ ! -f "$LLVM_DIR/LLVMConfig.cmake" ]]; then
    echo "LLVMConfig.cmake not found under LLVM_DIR=$LLVM_DIR" >&2
    echo "Build LLVM first with scripts/build_instrumented_llvm.sh or set LLVM_DIR." >&2
    exit 2
fi

cmake -S "$ROOT/fuzzer" -B "$FUZZER_BUILD_DIR" -G Ninja \
    -DLLVM_DIR="$LLVM_DIR" \
    -DFUZZER_SANITIZERS="$FUZZER_SANITIZERS" \
    -DCMAKE_BUILD_TYPE="$CMAKE_BUILD_TYPE" \
    -DCMAKE_C_COMPILER="${CC:-clang}" \
    -DCMAKE_CXX_COMPILER="${CXX:-clang++}"

cmake --build "$FUZZER_BUILD_DIR" --target llvm_spirv_crash_fuzzer \
    --parallel "${NINJAJOBS:-$(nproc)}"

echo "$FUZZER_BUILD_DIR/llvm_spirv_crash_fuzzer"
