#!/usr/bin/env bash
# Run the GPU differential fuzzer against ROCm 7.2.3 release semantics.
#
# This mode re-enables known idioms that reproduce only on LLVM HEAD / ROCm HEAD,
# while keeping ROCm 7.2.3 release failures suppressed.

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

FUZZER_BIN="${FUZZER_BIN:-$ROOT/build/fuzzer-rocm-7.2.3-cov-release/llvm_amdgpu_diff_fuzzer}"
CORPUS_ROOT="${CORPUS_ROOT:-$ROOT/corpus/rocm-7.2.3-cov-release-fuzz}"
ARTIFACT_ROOT="${ARTIFACT_ROOT:-$ROOT/findings/rocm-7.2.3-cov-release-fuzz-artifacts}"
FUZZX_FINDINGS_DIR="${FUZZX_FINDINGS_DIR:-$ROOT/findings/rocm-7.2.3-cov-release-fuzz}"
LOG_DIR="${LOG_DIR:-$ROOT/build/rocm-7.2.3-cov-release-fuzz-logs/$(date +%Y%m%d-%H%M%S)}"
GPUS="${GPUS:-0 1 2 3 4 5 6 7}"
WORKERS_PER_GPU="${WORKERS_PER_GPU:-32}"

# These known cases are HEAD-only for the checked toolchain matrix and should
# stay enabled while fuzzing the ROCm release.
export FUZZX_ALLOW_M002_I8_CLEAR_XOR=1
export FUZZX_ALLOW_M003_SHL3_ADD_CHAIN=1
export FUZZX_ALLOW_M004_VECTOR_IDENTITY_XOR=1
export FUZZX_ALLOW_M005_SHL_ADD_CHAIN=1
export FUZZX_ALLOW_M006_I8_CLEAR_XOR=1
export FUZZX_ALLOW_M007_VECTOR_IDENTITY_XOR=1
export FUZZX_ALLOW_M008_I8_CLEAR_XOR=1
export FUZZX_ALLOW_M009_I16_CLEAR_XOR=1
export FUZZX_ALLOW_M010_I16_SEXT_CLEAR_XOR=1
export FUZZX_ALLOW_M011_I8_SEXT_CLEAR_XOR=1
export FUZZX_ALLOW_M012_ADD_SHL_LADDER=1
export FUZZX_ALLOW_M014_SHL_ADD_CTPOP=1
export FUZZX_ALLOW_M015_SCALAR_FSHL_ZERO=1
export FUZZX_ALLOW_M016_SCALAR_FSHL=1

export FUZZER_BIN CORPUS_ROOT ARTIFACT_ROOT FUZZX_FINDINGS_DIR LOG_DIR GPUS
export WORKERS_PER_GPU

exec "$ROOT/scripts/run_directed_multigpu_fuzzer.sh" "$@"
