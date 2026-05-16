#!/usr/bin/env bash
# Run the C++ coverage-guided, GPU-executing AMDGPU differential fuzzer.

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

FUZZER_BIN="${FUZZER_BIN:-$ROOT/build/directed-fuzzer/llvm_amdgpu_diff_fuzzer}"
CORPUS_DIR="${CORPUS_DIR:-$ROOT/corpus/directed}"
ARTIFACT_DIR="${ARTIFACT_DIR:-$ROOT/findings/directed-artifacts}"
FUZZX_FINDINGS_DIR="${FUZZX_FINDINGS_DIR:-$ROOT/findings}"
ASAN_OPTIONS="${ASAN_OPTIONS:-detect_leaks=0}"

if [[ ! -x "$FUZZER_BIN" ]]; then
    echo "fuzzer binary not found: $FUZZER_BIN" >&2
    echo "Run scripts/build_directed_fuzzer.sh first." >&2
    exit 2
fi

mkdir -p "$CORPUS_DIR" "$ARTIFACT_DIR" "$FUZZX_FINDINGS_DIR"
if ! compgen -G "$CORPUS_DIR/*" >/dev/null; then
    printf '\001\002\003\004\005\006\007\010' >"$CORPUS_DIR/seed"
fi

export FUZZX_FINDINGS_DIR
export ASAN_OPTIONS

exec "$FUZZER_BIN" "$CORPUS_DIR" \
    -artifact_prefix="$ARTIFACT_DIR/" \
    "$@"
