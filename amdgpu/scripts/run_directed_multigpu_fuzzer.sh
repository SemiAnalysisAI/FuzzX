#!/usr/bin/env bash
# Run directed C++ AMDGPU differential fuzzer processes across selected GPUs.

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

USER_NAME="${USER:-$(id -u)}"
RUNTIME_ROOT="${FUZZX_RUNTIME_ROOT:-${TMPDIR:-/tmp}/fuzzx-amdgpu-$USER_NAME}"
FUZZER_BIN="${FUZZER_BIN:-$ROOT/build/fuzzer/llvm_amdgpu_diff_fuzzer}"
GPUS="${GPUS:-0 1 2 3 4 5 6 7}"
WORKERS_PER_GPU="${WORKERS_PER_GPU:-1}"
CORPUS_ROOT="${CORPUS_ROOT:-$RUNTIME_ROOT/corpus/directed-gpu}"
ARTIFACT_ROOT="${ARTIFACT_ROOT:-$RUNTIME_ROOT/artifacts/directed-gpu}"
LOG_DIR="${LOG_DIR:-$RUNTIME_ROOT/logs/directed-gpu/$(date +%Y%m%d-%H%M%S)}"
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

read -r -a GPU_LIST <<< "$GPUS"
if [[ "${#GPU_LIST[@]}" -eq 0 ]]; then
    echo "GPUS is empty" >&2
    exit 2
fi

if ! [[ "$WORKERS_PER_GPU" =~ ^[1-9][0-9]*$ ]]; then
    echo "WORKERS_PER_GPU must be a positive integer" >&2
    exit 2
fi

mkdir -p "$CORPUS_ROOT" "$ARTIFACT_ROOT" "$LOG_DIR" "$FUZZX_FINDINGS_DIR" "$TMPDIR"
export TMPDIR
export FUZZX_FINDINGS_DIR
export ASAN_OPTIONS

start_seconds="$SECONDS"
status=0
for device in "${GPU_LIST[@]}"; do
    for ((worker = 0; worker < WORKERS_PER_GPU; ++worker)); do
        if [[ "$WORKERS_PER_GPU" -eq 1 ]]; then
            name="device-$device"
        else
            name="device-$device-worker-$worker"
        fi
        corpus="$CORPUS_ROOT/$name"
        artifacts="$ARTIFACT_ROOT/$name"
        mkdir -p "$corpus" "$artifacts"
        if ! compgen -G "$corpus/*" >/dev/null; then
            printf '\001\002\003\004\005\006\007\010' >"$corpus/seed"
        fi
        HIP_DEVICE="$device" "$FUZZER_BIN" "$corpus" \
            -artifact_prefix="$artifacts/" \
            "$@" >"$LOG_DIR/$name.log" 2>&1 &
    done
done

for job in $(jobs -p); do
    wait "$job" || status=1
done

elapsed="$((SECONDS - start_seconds))"
echo "logs: $LOG_DIR"
echo "corpus: $CORPUS_ROOT"
echo "artifacts: $ARTIFACT_ROOT"
echo "findings: $FUZZX_FINDINGS_DIR"
echo "tmp: $TMPDIR"
echo "fuzzer: $FUZZER_BIN"
echo "devices: ${#GPU_LIST[@]}"
echo "workers_per_gpu: $WORKERS_PER_GPU"
echo "workers: $(("${#GPU_LIST[@]}" * WORKERS_PER_GPU))"
echo "elapsed_seconds: $elapsed"
for device in "${GPU_LIST[@]}"; do
    for ((worker = 0; worker < WORKERS_PER_GPU; ++worker)); do
        if [[ "$WORKERS_PER_GPU" -eq 1 ]]; then
            name="device-$device"
        else
            name="device-$device-worker-$worker"
        fi
        log="$LOG_DIR/$name.log"
        printf '%s tail="%s"\n' "$name" "$(tail -n 1 "$log")"
    done
done

exit "$status"
