#!/usr/bin/env bash
# Configure and build an LLVM tree suitable for coverage-guided AMDGPU fuzzing.
#
# Optional:
#   LLVM_PROJECT_DIR=/path/to/llvm-project
#   LLVM_BUILD_DIR=$PWD/build/llvm-fuzzer
#   LLVM_INSTALL_DIR=$PWD/build/llvm-fuzzer-install
#   LLVM_TARGETS_TO_BUILD='AMDGPU;X86'

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

LLVM_PROJECT_DIR="${LLVM_PROJECT_DIR:-$ROOT/third_party/llvm-project}"

if [[ ! -d "$LLVM_PROJECT_DIR/llvm" ]]; then
    echo "LLVM source checkout not found under LLVM_PROJECT_DIR=$LLVM_PROJECT_DIR" >&2
    echo "Run: git submodule update --init --depth 1 third_party/llvm-project" >&2
    exit 2
fi

LLVM_PROJECT_DIR="$(cd "$LLVM_PROJECT_DIR" && pwd)"
LLVM_BUILD_DIR="${LLVM_BUILD_DIR:-$ROOT/build/llvm-fuzzer}"
LLVM_INSTALL_DIR="${LLVM_INSTALL_DIR:-$ROOT/build/llvm-fuzzer-install}"
LLVM_TARGETS_TO_BUILD="${LLVM_TARGETS_TO_BUILD:-AMDGPU;X86}"

HOST_CLANG="${HOST_CLANG:-/opt/rocm-7.1.1/lib/llvm/bin/clang}"
HOST_CLANGXX="${HOST_CLANGXX:-/opt/rocm-7.1.1/lib/llvm/bin/clang++}"

cmake -S "$LLVM_PROJECT_DIR/llvm" -B "$LLVM_BUILD_DIR" -G Ninja \
    -DCMAKE_BUILD_TYPE=RelWithDebInfo \
    -DCMAKE_C_COMPILER="$HOST_CLANG" \
    -DCMAKE_CXX_COMPILER="$HOST_CLANGXX" \
    -DCMAKE_INSTALL_PREFIX="$LLVM_INSTALL_DIR" \
    -DLLVM_ENABLE_PROJECTS="clang;lld" \
    -DLLVM_TARGETS_TO_BUILD="$LLVM_TARGETS_TO_BUILD" \
    -DLLVM_ENABLE_ASSERTIONS=ON \
    -DLLVM_USE_SANITIZER=Address \
    -DLLVM_USE_SANITIZE_COVERAGE=ON \
    -DLLVM_LINK_LLVM_DYLIB=OFF \
    -DBUILD_SHARED_LIBS=OFF

cmake --build "$LLVM_BUILD_DIR" --target clang lld llvm-objdump --parallel "${NINJAJOBS:-$(nproc)}"

cat <<EOF
LLVM fuzzer toolchain built in:
  $LLVM_BUILD_DIR

Use it with:
  LLVM_BUILD_DIR=$LLVM_BUILD_DIR scripts/build_directed_fuzzer.sh
  scripts/run_directed_fuzzer.sh -runs=10000
EOF
