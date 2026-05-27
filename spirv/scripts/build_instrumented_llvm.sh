#!/usr/bin/env bash
# Configure and build an LLVM tree suitable for coverage-guided SPIR-V
# backend crash fuzzing.  Mirrors amdgpu/scripts/build_instrumented_llvm.sh,
# with HIP/AMDGPU specifics removed.
#
# Optional:
#   LLVM_PROJECT_DIR=/path/to/llvm-project
#   LLVM_BUILD_DIR=$PWD/build/llvm-fuzzer
#   LLVM_INSTALL_DIR=$PWD/build/llvm-fuzzer-install
#   LLVM_TARGETS_TO_BUILD='SPIRV;X86'
#   LLVM_ENABLE_ASSERTIONS=ON
#   LLVM_USE_SANITIZER=OFF
#   LLVM_USE_SANITIZE_COVERAGE=ON
#   LLVM_FUZZX_SANCOV=ON
#   CMAKE_BUILD_TYPE=Release
#   HOST_CLANG / HOST_CLANGXX

set -euo pipefail

cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.."
ROOT="$(pwd)"

LLVM_PROJECT_DIR="${LLVM_PROJECT_DIR:-$ROOT/third_party/llvm-project}"

if [[ ! -d "$LLVM_PROJECT_DIR/llvm" ]]; then
    echo "LLVM source checkout not found under LLVM_PROJECT_DIR=$LLVM_PROJECT_DIR" >&2
    echo "Run: git clone --depth 1 https://github.com/llvm/llvm-project $LLVM_PROJECT_DIR" >&2
    exit 2
fi

LLVM_PROJECT_DIR="$(cd "$LLVM_PROJECT_DIR" && pwd)"
LLVM_BUILD_DIR="${LLVM_BUILD_DIR:-$ROOT/build/llvm-fuzzer}"
LLVM_INSTALL_DIR="${LLVM_INSTALL_DIR:-$ROOT/build/llvm-fuzzer-install}"
LLVM_TARGETS_TO_BUILD="${LLVM_TARGETS_TO_BUILD:-SPIRV;X86}"
# Assertions on so we catch backend ICEs, the whole point of the fuzzer.
LLVM_ENABLE_ASSERTIONS="${LLVM_ENABLE_ASSERTIONS:-ON}"
LLVM_USE_SANITIZER="${LLVM_USE_SANITIZER:-OFF}"
LLVM_USE_SANITIZE_COVERAGE="${LLVM_USE_SANITIZE_COVERAGE:-ON}"
# LLVM_FUZZX_SANCOV=ON injects -fsanitize-coverage=... into CMAKE_*_FLAGS so
# every LLVM TU gets sancov even when LLVM_USE_SANITIZER is OFF.
LLVM_FUZZX_SANCOV="${LLVM_FUZZX_SANCOV:-ON}"
CMAKE_BUILD_TYPE="${CMAKE_BUILD_TYPE:-Release}"

HOST_CLANG="${HOST_CLANG:-${CC:-clang}}"
HOST_CLANGXX="${HOST_CLANGXX:-${CXX:-clang++}}"

cmake_args=(
    -S "$LLVM_PROJECT_DIR/llvm"
    -B "$LLVM_BUILD_DIR"
    -G Ninja
    -DCMAKE_BUILD_TYPE="$CMAKE_BUILD_TYPE"
    -DCMAKE_C_COMPILER="$HOST_CLANG"
    -DCMAKE_CXX_COMPILER="$HOST_CLANGXX"
    -DCMAKE_INSTALL_PREFIX="$LLVM_INSTALL_DIR"
    -DLLVM_ENABLE_PROJECTS=""
    -DLLVM_TARGETS_TO_BUILD="$LLVM_TARGETS_TO_BUILD"
    -DLLVM_ENABLE_ASSERTIONS="$LLVM_ENABLE_ASSERTIONS"
    -DLLVM_USE_SANITIZE_COVERAGE="$LLVM_USE_SANITIZE_COVERAGE"
    -DLLVM_LINK_LLVM_DYLIB=OFF
    -DBUILD_SHARED_LIBS=OFF
)

if [[ -n "$LLVM_USE_SANITIZER" && "$LLVM_USE_SANITIZER" != "OFF" ]]; then
    cmake_args+=(-DLLVM_USE_SANITIZER="$LLVM_USE_SANITIZER")
else
    cmake_args+=(-DLLVM_USE_SANITIZER=)
fi

if [[ "$LLVM_FUZZX_SANCOV" =~ ^(1|ON|on|true|TRUE|yes|YES)$ ]]; then
    SANCOV_FLAGS="-fsanitize-coverage=inline-8bit-counters,pc-table"
    cmake_args+=(
        -DCMAKE_C_FLAGS_INIT="$SANCOV_FLAGS"
        -DCMAKE_CXX_FLAGS_INIT="$SANCOV_FLAGS"
    )
fi

cmake "${cmake_args[@]}"

cmake --build "$LLVM_BUILD_DIR" \
    --target llc llvm-stress opt llvm-as \
    --parallel "${NINJAJOBS:-$(nproc)}"

cat <<EOF
LLVM fuzzer toolchain built in:
  $LLVM_BUILD_DIR

Use it with:
  LLVM_BUILD_DIR=$LLVM_BUILD_DIR scripts/build_directed_fuzzer.sh
  scripts/run_directed_fuzzer.sh -runs=10000
EOF
