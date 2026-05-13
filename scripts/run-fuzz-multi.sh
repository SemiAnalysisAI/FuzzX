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
# The trim stage is the worst-affected by the residual AFL heap bug
# in 4.41a — disabling it dramatically reduces worker mortality at
# >=50 cores. Trim only matters for shrinking accepted corpus entries,
# which is mostly cosmetic for our use case.
export AFL_DISABLE_TRIM=1

mkdir -p "$OUT_DIR"
LOG_DIR="$OUT_DIR/logs"
mkdir -p "$LOG_DIR"

echo "ptx-fuzz: starting $CORES workers against $PTXAS"
echo "          mutator: $LIB"
echo "          seeds:   $SEEDS_DIR"
echo "          output:  $OUT_DIR"
[[ -n "$RUNTIME" ]] && echo "          runtime: ${RUNTIME}s (then kill)"

declare -A worker_pid

start_worker() {
    local name="$1"
    local mode="$2"  # "-M" for master, "-S" for secondary
    # For an existing output dir we have to use afl-fuzz's "resume"
    # flag (`-i -`) so AFL picks up where it left off instead of
    # complaining that the dir is non-empty.
    local in_flag="-i $SEEDS_DIR"
    if [[ -d "$OUT_DIR/$name/queue" ]]; then in_flag="-i -"; fi
    afl-fuzz -Q $mode "$name" \
        $in_flag -o "$OUT_DIR" -t "$TIMEOUT_MS" \
        -- "$PTXAS" "@@" > "$LOG_DIR/$name.log" 2>&1 &
    worker_pid["$name"]=$!
}

cleanup() {
    echo "ptx-fuzz: stopping workers..."
    for p in "${worker_pid[@]}"; do
        kill "$p" 2>/dev/null || true
    done
    wait 2>/dev/null || true
}
trap cleanup INT TERM EXIT

start_worker main "-M"
for (( i = 1; i < CORES; i++ )); do
    name=$(printf 'sec%02d' "$i")
    start_worker "$name" "-S"
done

echo "ptx-fuzz: workers running; logs in $LOG_DIR"
echo "ptx-fuzz: live status:  afl-whatsup -s $OUT_DIR"
echo "ptx-fuzz: crashes:      ls $OUT_DIR/*/crashes/"

# Watchdog. AFL++ workers occasionally segfault inside libc under
# sustained runs (see DESIGN.md "Known issues"). Without this we'd
# steadily lose throughput; with it we keep all $CORES slots filled.
#
# Crucially, the watchdog throttles per-worker restarts. At very high
# worker counts the system can enter a death-spiral where dozens of
# workers die at once, the watchdog restarts them all simultaneously,
# they re-die during calibration, and the cycle continues. To avoid
# that, each worker tracks its last-restart time and waits a
# back-off interval (doubling, up to 5 min) before being restarted.
end_time=0
if [[ -n "$RUNTIME" ]]; then end_time=$(( $(date +%s) + RUNTIME )); fi

declare -A worker_last_restart
declare -A worker_backoff
for name in "${!worker_pid[@]}"; do
    worker_last_restart["$name"]=0
    worker_backoff["$name"]=10
done

while true; do
    if [[ -n "$RUNTIME" ]] && (( $(date +%s) >= end_time )); then
        echo "ptx-fuzz: runtime budget elapsed (${RUNTIME}s)"
        break
    fi
    now=$(date +%s)
    for name in "${!worker_pid[@]}"; do
        pid="${worker_pid[$name]}"
        if ! kill -0 "$pid" 2>/dev/null; then
            since=$(( now - ${worker_last_restart[$name]:-0} ))
            backoff=${worker_backoff[$name]:-10}
            if (( since < backoff )); then continue; fi
            mode="-S"
            [[ "$name" == "main" ]] && mode="-M"
            echo "ptx-fuzz: $name (pid $pid) died; restarting (backoff was ${backoff}s)"
            start_worker "$name" "$mode"
            worker_last_restart["$name"]=$now
            # If this worker has been thrashing (died within the last
            # 3*backoff seconds), grow its next backoff; otherwise
            # reset it.
            if (( since < backoff * 3 )); then
                next=$(( backoff * 2 ))
                (( next > 300 )) && next=300
                worker_backoff["$name"]=$next
            else
                worker_backoff["$name"]=10
            fi
        fi
    done
    sleep 5
done
