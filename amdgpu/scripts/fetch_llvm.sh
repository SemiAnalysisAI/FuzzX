#!/usr/bin/env bash
# Fetch a shallow LLVM source checkout for the directed AMDGPU backend fuzzer.

set -euo pipefail

cd "$(dirname "$0")/.."

LLVM_REPO="${LLVM_REPO:-https://github.com/llvm/llvm-project.git}"
LLVM_BRANCH="${LLVM_BRANCH:-main}"
LLVM_PROJECT_DIR="${LLVM_PROJECT_DIR:-$PWD/third_party/llvm-project}"

if [[ -d "$LLVM_PROJECT_DIR/.git" ]]; then
    echo "LLVM checkout already exists: $LLVM_PROJECT_DIR"
    git -C "$LLVM_PROJECT_DIR" rev-parse --short HEAD
    exit 0
fi

mkdir -p "$(dirname "$LLVM_PROJECT_DIR")"
git clone --depth 1 --filter=blob:none --single-branch --branch "$LLVM_BRANCH" \
    "$LLVM_REPO" "$LLVM_PROJECT_DIR"
git -C "$LLVM_PROJECT_DIR" rev-parse HEAD
