#!/usr/bin/env bash
# Build the in-process, GPU-executing AMDGPU differential libFuzzer target.
#
# Required unless using the default instrumented build path:
#   LLVM_DIR=/path/to/llvm-build/lib/cmake/llvm
#   LLD_DIR=/path/to/llvm-build/lib/cmake/lld
#
# Optional:
#   FUZZER_SANITIZERS=fuzzer,address
#   CMAKE_BUILD_TYPE=Release

set -euo pipefail

cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.."
ROOT="$(pwd)"

LLVM_BUILD_DIR="${LLVM_BUILD_DIR:-$ROOT/build/llvm-fuzzer}"
LLVM_DIR="${LLVM_DIR:-$LLVM_BUILD_DIR/lib/cmake/llvm}"
LLD_DIR="${LLD_DIR:-$LLVM_BUILD_DIR/lib/cmake/lld}"
FUZZER_BUILD_DIR="${FUZZER_BUILD_DIR:-$ROOT/build/fuzzer}"
ROCM_PATH="${ROCM_PATH:-/opt/rocm-7.1.1}"
FUZZER_SANITIZERS="${FUZZER_SANITIZERS:-fuzzer}"
CMAKE_BUILD_TYPE="${CMAKE_BUILD_TYPE:-Release}"

if [[ ! -f "$LLVM_DIR/LLVMConfig.cmake" ]]; then
    echo "LLVMConfig.cmake not found under LLVM_DIR=$LLVM_DIR" >&2
    echo "Build LLVM first with scripts/build_instrumented_llvm.sh or set LLVM_DIR." >&2
    exit 2
fi

if [[ ! -f "$LLD_DIR/LLDConfig.cmake" ]]; then
    echo "LLDConfig.cmake not found under LLD_DIR=$LLD_DIR" >&2
    echo "Build LLVM/lld first with scripts/build_instrumented_llvm.sh or set LLD_DIR." >&2
    exit 2
fi

if [[ -f "$LLVM_BUILD_DIR/CMakeCache.txt" ]]; then
    if [[ ! -f "$LLVM_BUILD_DIR/lib/libLLVMExecutionEngine.a" || \
          ! -f "$LLVM_BUILD_DIR/lib/libLLVMInterpreter.a" ]]; then
        cmake --build "$LLVM_BUILD_DIR" \
            --target LLVMExecutionEngine LLVMInterpreter \
            --parallel "${NINJAJOBS:-$(nproc)}"
    fi
fi

cmake -S "$ROOT/fuzzer" -B "$FUZZER_BUILD_DIR" -G Ninja \
    -DLLVM_DIR="$LLVM_DIR" \
    -DLLD_DIR="$LLD_DIR" \
    -DROCM_PATH="$ROCM_PATH" \
    -DFUZZER_SANITIZERS="$FUZZER_SANITIZERS" \
    -DCMAKE_BUILD_TYPE="$CMAKE_BUILD_TYPE" \
    -DCMAKE_C_COMPILER="${CC:-$ROCM_PATH/lib/llvm/bin/clang}" \
    -DCMAKE_CXX_COMPILER="${CXX:-$ROCM_PATH/lib/llvm/bin/clang++}"

cmake --build "$FUZZER_BUILD_DIR" --target llvm_amdgpu_diff_fuzzer --parallel "${NINJAJOBS:-$(nproc)}"

echo "$FUZZER_BUILD_DIR/llvm_amdgpu_diff_fuzzer"
