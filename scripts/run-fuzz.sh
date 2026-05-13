#!/usr/bin/env bash
# Drive an AFL++ qemu_mode fuzzing run against ptxas.
#
# Required on $PATH: afl-fuzz, afl-qemu-trace (from AFL++).
# Required env / defaults:
#   PTXAS      target binary       (default: ptxas in $PATH)
#   SEEDS_DIR  AFL seed corpus     (default: ../seeds, relative to this script)
#   OUT_DIR    AFL output dir      (default: ../output)
#   TIMEOUT_MS per-iter hang limit (default: 5000)
#
# This script just sets the env that AFL++ expects and exec's afl-fuzz.
# Stop the run with Ctrl-C; AFL writes crashes to $OUT_DIR/default/crashes/.

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

# Non-interactive shells (e.g. `gcloud compute ssh --command`) skip
# the rustup shell hook, so PATH may not include ~/.cargo/bin.
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

# Build the mutator (release, debug info on so crashes stay debuggable).
cargo build --release -p ptx-fuzz-mutator

# Pick the right shared-library extension for the host platform. AFL++
# qemu_mode only really works on Linux, so .so is the common case, but
# we keep the macOS path for local-syntax-checking convenience.
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
# We *want* AFL's built-in mutators to keep running on the raw byte
# input; our post_process hook only transforms bytes → PTX text just
# before they reach ptxas. So do NOT set AFL_CUSTOM_MUTATOR_ONLY=1.

# ptxas isn't built with AFL instrumentation — coverage comes from
# qemu_mode patching translated blocks at runtime.
export AFL_SKIP_BIN_CHECK=1

# AFL++ 4.41a's FrameShift stage corrupts the heap when combined with a
# post_process custom mutator (reproduced on stock AFL++ master,
# 2026-05). Crash trace: malloc() invalid size in frameshift_stage,
# afl-fuzz-frameshift.c:469. Disable it for now.
export AFL_FRAMESHIFT_DISABLE=1

# Default qemu_mode forkserver hangs at ptxas's entry point. That's
# usually fine; uncomment to set a specific entrypoint for persistent
# mode (a future optimization — would need ptxas's main address).
# export AFL_QEMU_PERSISTENT_ADDR=0x...

echo "ptx-fuzz: starting afl-fuzz -Q against $PTXAS"
echo "          mutator: $LIB"
echo "          seeds:   $SEEDS_DIR"
echo "          output:  $OUT_DIR"

exec afl-fuzz -Q \
    -i "$SEEDS_DIR" \
    -o "$OUT_DIR" \
    -t "$TIMEOUT_MS" \
    -- "$PTXAS" "@@"
