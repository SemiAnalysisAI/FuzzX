#!/usr/bin/env bash
# Run the GPU differential fuzzer against the ROCm 7.2.3 release build.

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

USER_NAME="${USER:-$(id -u)}"
RUNTIME_ROOT="${FUZZX_RUNTIME_ROOT:-${TMPDIR:-/tmp}/fuzzx-amdgpu-$USER_NAME}"
RUN_STAMP="${FUZZX_RUN_ID:-$(date +%Y%m%d-%H%M%S)}"
FUZZER_BIN="${FUZZER_BIN:-$ROOT/build/fuzzer-rocm-7.2.3-cov-release/llvm_amdgpu_diff_fuzzer}"
CORPUS_ROOT="${CORPUS_ROOT:-$RUNTIME_ROOT/corpus/rocm-7.2.3-cov-release-fuzz}"
ARTIFACT_ROOT="${ARTIFACT_ROOT:-$RUNTIME_ROOT/artifacts/rocm-7.2.3-cov-release-fuzz}"
FUZZX_FINDINGS_DIR="${FUZZX_FINDINGS_DIR:-$RUNTIME_ROOT/findings/rocm-7.2.3-cov-release-fuzz}"
LOG_DIR="${LOG_DIR:-$RUNTIME_ROOT/logs/rocm-7.2.3-cov-release-fuzz/$RUN_STAMP}"
GPUS="${GPUS:-0 1 2 3 4 5 6 7}"
WORKERS_PER_GPU="${WORKERS_PER_GPU:-32}"

export FUZZER_BIN CORPUS_ROOT ARTIFACT_ROOT FUZZX_FINDINGS_DIR LOG_DIR GPUS
export WORKERS_PER_GPU

exec "$ROOT/scripts/run_directed_multigpu_fuzzer.sh" "$@"
