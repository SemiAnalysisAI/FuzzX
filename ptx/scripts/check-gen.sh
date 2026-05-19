#!/usr/bin/env bash
# Measure how often the differential generator produces PTX that assembles.
#
# Usage:
#   PTXAS=$(which ptxas) scripts/check-gen.sh [N]   # default N=200
#
# Output: pass rate + a histogram of the first error line for each
# failing input. A healthy grammar generator should pass >99%; lower
# rates indicate a typing-rule regression somewhere in the menus.

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

N="${1:-200}"
PTXAS="${PTXAS:-$(command -v ptxas)}"
if [[ -z "$PTXAS" ]]; then echo "ptxas not on PATH; set \$PTXAS" >&2; exit 1; fi
PTXAS_ARCH="${PTXAS_ARCH:-sm_103}"

GEN="$ROOT/target/release/fuzzx-diff-dump-gen"
if [[ ! -x "$GEN" ]]; then
    cargo build --release -p fuzzx-diff --bin fuzzx-diff-dump-gen >/dev/null
fi
START_SEED="${DIV_STARTING_SEED:-0}"

declare -A errs
pass=0
tmp_ptx=$(mktemp --suffix=.ptx)
tmp_cubin=$(mktemp --suffix=.cubin)
trap 'rm -f "$tmp_ptx" "$tmp_cubin"' EXIT

for ((i = 0; i < N; i++)); do
    seed=$((START_SEED + i))
    "$GEN" "$seed" > "$tmp_ptx"
    if out=$("$PTXAS" "-arch=$PTXAS_ARCH" "$tmp_ptx" -o "$tmp_cubin" 2>&1 >/dev/null); then
        pass=$((pass + 1))
        err="(PASS)"
    else
        err=$(printf '%s\n' "$out" \
            | head -1 \
            | sed 's#/tmp/[^ ,:]*##g; s/, line [0-9]*//' \
            | cut -c1-80)
        if [[ -z "$err" ]]; then
            err="(FAILED with empty stderr)"
        fi
    fi
    errs[$err]=$(( ${errs[$err]:-0} + 1 ))
done

printf 'pass: %d / %d (%d%%)\n' "$pass" "$N" "$(( pass * 100 / N ))"
echo "--- error histogram ---"
# `head -15` closes the pipe before `sort` finishes; with pipefail
# that's a script-level failure. Disable pipefail just for the summary.
set +o pipefail
for k in "${!errs[@]}"; do
    printf '%4d  %s\n' "${errs[$k]}" "$k"
done | sort -rn | head -15
