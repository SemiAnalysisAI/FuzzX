#!/usr/bin/env bash
# Run the C++ coverage-guided, in-process SPIR-V backend crash fuzzer.
# Mirrors amdgpu/scripts/run_directed_fuzzer.sh, with GPU/CPU-pinning bits
# preserved.  Crash artifacts are saved by libFuzzer to ARTIFACT_DIR; this
# fuzzer additionally writes IR/.bc copies to FUZZX_FINDINGS_DIR.

set -euo pipefail

cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.."
ROOT="$(pwd)"

USER_NAME="${USER:-$(id -u)}"
RUNTIME_ROOT="${FUZZX_RUNTIME_ROOT:-${TMPDIR:-/tmp}/fuzzx-spirv-$USER_NAME}"
FUZZER_BIN="${FUZZER_BIN:-$ROOT/build/fuzzer/llvm_spirv_crash_fuzzer}"
CORPUS_DIR="${CORPUS_DIR:-$RUNTIME_ROOT/corpus/directed}"
ARTIFACT_DIR="${ARTIFACT_DIR:-$RUNTIME_ROOT/artifacts/directed}"
FUZZX_FINDINGS_DIR="${FUZZX_FINDINGS_DIR:-$RUNTIME_ROOT/findings}"
TMPDIR="${FUZZX_TMPDIR:-$RUNTIME_ROOT/tmp}"
FUZZX_LOCALIZE_FUZZER="${FUZZX_LOCALIZE_FUZZER:-1}"
ASAN_OPTIONS="${ASAN_OPTIONS:-detect_leaks=0}"

if [[ ! -x "$FUZZER_BIN" ]]; then
    echo "fuzzer binary not found: $FUZZER_BIN" >&2
    echo "Run scripts/build_directed_fuzzer.sh first." >&2
    exit 2
fi

if [[ "$FUZZX_LOCALIZE_FUZZER" != "0" && "$FUZZX_LOCALIZE_FUZZER" != "false" ]]; then
    mkdir -p "$RUNTIME_ROOT/bin"
    fuzzer_key="$(printf '%s' "$FUZZER_BIN" | cksum | awk '{print $1}')"
    local_fuzzer_bin="$RUNTIME_ROOT/bin/$(basename "$FUZZER_BIN")-$fuzzer_key"
    src_size="$(stat -c '%s' "$FUZZER_BIN")"
    dst_size="$(stat -c '%s' "$local_fuzzer_bin" 2>/dev/null || echo -1)"
    if [[ ! -x "$local_fuzzer_bin" || "$FUZZER_BIN" -nt "$local_fuzzer_bin" || "$src_size" != "$dst_size" ]]; then
        cp -f "$FUZZER_BIN" "$local_fuzzer_bin.tmp"
        chmod +x "$local_fuzzer_bin.tmp"
        mv -f "$local_fuzzer_bin.tmp" "$local_fuzzer_bin"
    fi
    FUZZER_BIN="$local_fuzzer_bin"
fi

mkdir -p "$CORPUS_DIR" "$ARTIFACT_DIR" "$FUZZX_FINDINGS_DIR" "$TMPDIR"
"$ROOT/scripts/seed_ir_corpus.sh" "$CORPUS_DIR"

export TMPDIR
export FUZZX_FINDINGS_DIR
export ASAN_OPTIONS

exec "$FUZZER_BIN" "$CORPUS_DIR" \
    -artifact_prefix="$ARTIFACT_DIR/" \
    "$@"
