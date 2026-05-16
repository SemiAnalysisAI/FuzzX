#!/usr/bin/env bash
# Run one directed C++ AMDGPU differential fuzzer process per selected GPU.

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

FUZZER_BIN="${FUZZER_BIN:-$ROOT/build/directed-fuzzer/llvm_amdgpu_diff_fuzzer}"
GPUS="${GPUS:-0 1 2 3 4 5 6 7}"
CORPUS_ROOT="${CORPUS_ROOT:-$ROOT/corpus/directed-gpu}"
ARTIFACT_ROOT="${ARTIFACT_ROOT:-$ROOT/findings/directed-artifacts}"
LOG_DIR="${LOG_DIR:-$ROOT/build/directed-logs/$(date +%Y%m%d-%H%M%S)}"
FUZZX_FINDINGS_DIR="${FUZZX_FINDINGS_DIR:-$ROOT/findings}"
ASAN_OPTIONS="${ASAN_OPTIONS:-detect_leaks=0}"

if [[ ! -x "$FUZZER_BIN" ]]; then
    echo "fuzzer binary not found: $FUZZER_BIN" >&2
    echo "Run scripts/build_directed_fuzzer.sh first." >&2
    exit 2
fi

read -r -a GPU_LIST <<< "$GPUS"
if [[ "${#GPU_LIST[@]}" -eq 0 ]]; then
    echo "GPUS is empty" >&2
    exit 2
fi

mkdir -p "$CORPUS_ROOT" "$ARTIFACT_ROOT" "$LOG_DIR" "$FUZZX_FINDINGS_DIR"
export FUZZX_FINDINGS_DIR
export ASAN_OPTIONS

start_seconds="$SECONDS"
status=0
for device in "${GPU_LIST[@]}"; do
    corpus="$CORPUS_ROOT/device-$device"
    artifacts="$ARTIFACT_ROOT/device-$device"
    mkdir -p "$corpus" "$artifacts"
    if ! compgen -G "$corpus/*" >/dev/null; then
        printf '\001\002\003\004\005\006\007\010' >"$corpus/seed"
    fi
    HIP_DEVICE="$device" "$FUZZER_BIN" "$corpus" \
        -artifact_prefix="$artifacts/" \
        "$@" >"$LOG_DIR/device-$device.log" 2>&1 &
done

for job in $(jobs -p); do
    wait "$job" || status=1
done

elapsed="$((SECONDS - start_seconds))"
echo "logs: $LOG_DIR"
echo "devices: ${#GPU_LIST[@]}"
echo "elapsed_seconds: $elapsed"
for device in "${GPU_LIST[@]}"; do
    log="$LOG_DIR/device-$device.log"
    printf 'device=%s tail="%s"\n' "$device" "$(tail -n 1 "$log")"
done

exit "$status"
