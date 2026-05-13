#!/usr/bin/env bash
# Multi-core AFL++ qemu_mode fuzzing run against ptxas.
#
# Same env vars as run-fuzz.sh, plus:
#   CORES   number of parallel fuzzers (default: min(nproc, 16))
#   RUNTIME if set, kill the run after this many seconds (e.g. 600)
#
# Architecture: one master (`-M main`) that owns deterministic
# stages, plus N-1 secondaries (`-S secN`) that hammer with random
# mutations. AFL++ syncs corpora across all instances via $OUT_DIR.
#
# Crashes from any worker land in $OUT_DIR/<worker>/crashes/. The
# `afl-whatsup` tool summarizes across workers.

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

if [[ -r "$HOME/.cargo/env" ]]; then
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
fi

PTXAS="${PTXAS:-$(command -v ptxas || true)}"
if [[ -z "$PTXAS" ]]; then
    echo "error: ptxas not found; set \$PTXAS or put it on \$PATH" >&2
    exit 1
fi

SEEDS_DIR="${SEEDS_DIR:-$ROOT/seeds}"
OUT_DIR="${OUT_DIR:-$ROOT/output}"
TIMEOUT_MS="${TIMEOUT_MS:-5000}"
CORES="${CORES:-}"
RUNTIME="${RUNTIME:-}"

if [[ -z "$CORES" ]]; then
    nproc_val=$(nproc 2>/dev/null || echo 1)
    if (( nproc_val > 16 )); then CORES=16; else CORES=$nproc_val; fi
fi
if (( CORES < 1 )); then CORES=1; fi

cargo build --release -p ptx-fuzz-mutator

case "$(uname -s)" in
    Linux)  LIB="$ROOT/target/release/libptx_fuzz_mutator.so" ;;
    Darwin) LIB="$ROOT/target/release/libptx_fuzz_mutator.dylib" ;;
    *) echo "unsupported host: $(uname -s)" >&2; exit 1 ;;
esac
if [[ ! -f "$LIB" ]]; then
    echo "error: mutator library not built at $LIB" >&2
    exit 1
fi

export AFL_CUSTOM_MUTATOR_LIBRARY="$LIB"
export AFL_SKIP_BIN_CHECK=1
# AFL++ 4.41a FrameShift stage corrupts heap with a post_process
# mutator — see run-fuzz.sh.
export AFL_FRAMESHIFT_DISABLE=1
# Workers run headless; only the main fuzzer logs to a TTY.
export AFL_NO_UI=1

mkdir -p "$OUT_DIR"
LOG_DIR="$OUT_DIR/logs"
mkdir -p "$LOG_DIR"

echo "ptx-fuzz: starting $CORES workers against $PTXAS"
echo "          mutator: $LIB"
echo "          seeds:   $SEEDS_DIR"
echo "          output:  $OUT_DIR"
[[ -n "$RUNTIME" ]] && echo "          runtime: ${RUNTIME}s (then kill)"

pids=()
cleanup() {
    echo "ptx-fuzz: stopping workers..."
    for p in "${pids[@]}"; do
        kill "$p" 2>/dev/null || true
    done
    wait 2>/dev/null || true
}
trap cleanup INT TERM EXIT

# First worker is the master.
afl-fuzz -Q -M main \
    -i "$SEEDS_DIR" -o "$OUT_DIR" -t "$TIMEOUT_MS" \
    -- "$PTXAS" "@@" > "$LOG_DIR/main.log" 2>&1 &
pids+=($!)

# Secondaries.
for (( i = 1; i < CORES; i++ )); do
    name=$(printf 'sec%02d' "$i")
    afl-fuzz -Q -S "$name" \
        -i "$SEEDS_DIR" -o "$OUT_DIR" -t "$TIMEOUT_MS" \
        -- "$PTXAS" "@@" > "$LOG_DIR/$name.log" 2>&1 &
    pids+=($!)
done

echo "ptx-fuzz: workers running (pids: ${pids[*]}); logs in $LOG_DIR"
echo "ptx-fuzz: live status:  afl-whatsup -s $OUT_DIR"
echo "ptx-fuzz: crashes:      ls $OUT_DIR/*/crashes/"

if [[ -n "$RUNTIME" ]]; then
    sleep "$RUNTIME"
    echo "ptx-fuzz: runtime budget elapsed (${RUNTIME}s)"
else
    wait
fi
