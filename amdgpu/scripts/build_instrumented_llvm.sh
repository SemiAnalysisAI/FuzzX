#!/usr/bin/env bash
# Configure and build an LLVM tree suitable for coverage-guided AMDGPU fuzzing.
#
# Optional:
#   LLVM_PROJECT_DIR=/path/to/llvm-project
#   LLVM_BUILD_DIR=$PWD/build/llvm-fuzzer
#   LLVM_INSTALL_DIR=$PWD/build/llvm-fuzzer-install
#   LLVM_TARGETS_TO_BUILD='AMDGPU;X86'
#   LLVM_ENABLE_ASSERTIONS=OFF
#   LLVM_USE_SANITIZER=OFF
#   LLVM_USE_SANITIZE_COVERAGE=ON
#   LLVM_APPLY_PR_198373=ON
#   LLVM_APPLY_PR_196418=ON
#   LLVM_APPLY_PR_198412=ON
#   LLVM_APPLY_PR_198419=ON
#   CMAKE_BUILD_TYPE=Release

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
LLVM_APPLY_PR_198373="${LLVM_APPLY_PR_198373:-ON}"
LLVM_APPLY_PR_196418="${LLVM_APPLY_PR_196418:-ON}"
LLVM_APPLY_PR_198412="${LLVM_APPLY_PR_198412:-ON}"
LLVM_APPLY_PR_198419="${LLVM_APPLY_PR_198419:-ON}"
LLVM_BUILD_DIR="${LLVM_BUILD_DIR:-$ROOT/build/llvm-fuzzer}"
LLVM_INSTALL_DIR="${LLVM_INSTALL_DIR:-$ROOT/build/llvm-fuzzer-install}"
LLVM_TARGETS_TO_BUILD="${LLVM_TARGETS_TO_BUILD:-AMDGPU;X86}"
LLVM_ENABLE_ASSERTIONS="${LLVM_ENABLE_ASSERTIONS:-OFF}"
LLVM_USE_SANITIZER="${LLVM_USE_SANITIZER:-OFF}"
LLVM_USE_SANITIZE_COVERAGE="${LLVM_USE_SANITIZE_COVERAGE:-ON}"
CMAKE_BUILD_TYPE="${CMAKE_BUILD_TYPE:-Release}"

HOST_CLANG="${HOST_CLANG:-/opt/rocm-7.1.1/lib/llvm/bin/clang}"
HOST_CLANGXX="${HOST_CLANGXX:-/opt/rocm-7.1.1/lib/llvm/bin/clang++}"

apply_optional_patch() {
    local label="$1"
    local enabled="$2"
    local patch_file="$3"

    if [[ ! "$enabled" =~ ^(1|ON|on|true|TRUE|yes|YES)$ ]]; then
        return
    fi
    if [[ ! -f "$patch_file" ]]; then
        echo "$label patch not found: $patch_file" >&2
        exit 2
    fi
    if git -C "$LLVM_PROJECT_DIR" apply --reverse --check "$patch_file" >/dev/null 2>&1; then
        echo "$label patch already applied in $LLVM_PROJECT_DIR"
    elif git -C "$LLVM_PROJECT_DIR" apply --check "$patch_file" >/dev/null 2>&1; then
        git -C "$LLVM_PROJECT_DIR" apply "$patch_file"
        echo "Applied $label patch in $LLVM_PROJECT_DIR"
    else
        echo "Cannot apply $label patch to LLVM_PROJECT_DIR=$LLVM_PROJECT_DIR" >&2
        echo "Set the corresponding LLVM_APPLY_PR_* variable to OFF if this checkout already contains an incompatible equivalent fix." >&2
        exit 2
    fi
}

apply_optional_patch "LLVM PR 198373" "$LLVM_APPLY_PR_198373" \
    "$ROOT/patches/llvm-pr-198373.diff"
apply_optional_patch "LLVM PR 196418" "$LLVM_APPLY_PR_196418" \
    "$ROOT/patches/llvm-pr-196418.diff"
apply_optional_patch "LLVM PR 198412" "$LLVM_APPLY_PR_198412" \
    "$ROOT/patches/llvm-pr-198412.diff"
apply_optional_patch "LLVM PR 198419" "$LLVM_APPLY_PR_198419" \
    "$ROOT/patches/llvm-pr-198419.diff"

cmake_args=(
    -S "$LLVM_PROJECT_DIR/llvm"
    -B "$LLVM_BUILD_DIR"
    -G Ninja
    -DCMAKE_BUILD_TYPE="$CMAKE_BUILD_TYPE"
    -DCMAKE_C_COMPILER="$HOST_CLANG"
    -DCMAKE_CXX_COMPILER="$HOST_CLANGXX"
    -DCMAKE_INSTALL_PREFIX="$LLVM_INSTALL_DIR"
    -DLLVM_ENABLE_PROJECTS="clang;lld"
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

cmake "${cmake_args[@]}"

cmake --build "$LLVM_BUILD_DIR" --target clang lld llvm-objdump --parallel "${NINJAJOBS:-$(nproc)}"

cat <<EOF
LLVM fuzzer toolchain built in:
  $LLVM_BUILD_DIR

Use it with:
  LLVM_BUILD_DIR=$LLVM_BUILD_DIR scripts/build_directed_fuzzer.sh
  scripts/run_directed_fuzzer.sh -runs=10000
EOF
