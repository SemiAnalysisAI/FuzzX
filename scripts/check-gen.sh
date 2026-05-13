#!/usr/bin/env bash
# Measure how often the generator produces PTX that assembles end-to-end.
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

REPRO="$ROOT/target/release/ptx-fuzz-repro"
if [[ ! -x "$REPRO" ]]; then cargo build --release -p ptx-fuzz-repro >/dev/null; fi

declare -A errs
pass=0
tmp_in=$(mktemp)
tmp_ptx=$(mktemp --suffix=.ptx)
trap 'rm -f "$tmp_in" "$tmp_ptx"' EXIT

for i in $(seq 1 "$N"); do
    head -c $(( RANDOM % 1024 + 8 )) /dev/urandom > "$tmp_in"
    "$REPRO" "$tmp_in" > "$tmp_ptx"
    err=$("$PTXAS" "$tmp_ptx" 2>&1 >/dev/null \
        | head -1 \
        | sed 's#/tmp/[^ ,:]*##g; s/, line [0-9]*//' \
        | cut -c1-80)
    if [[ -z "$err" ]]; then
        pass=$((pass + 1))
        err="(PASS)"
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
