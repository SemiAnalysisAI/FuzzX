#!/usr/bin/env bash
# Walk every crash AFL saved (across all worker dirs), re-run ptxas
# on each, and group inputs by their crashing signal + stderr summary.
#
# Output goes to $OUT_DIR/triage/:
#   summary.txt           - one line per crash group, with a count
#   group-<N>/             - per-group directory
#     example.bytes       - representative raw input
#     example.ptx         - PTX that ptxas actually saw
#     example.stderr      - ptxas's stderr for the representative
#     members.txt         - list of all raw-input paths in this group
#
# Two crashes are grouped if their ptxas exit code is the same and
# the last `error`/`fatal`/`Aborted` line of stderr matches. That's a
# crude heuristic but good enough to keep a few-bug pile from drowning
# in dups during initial triage.
#
# Usage:
#   PTXAS=$(which ptxas) scripts/triage.sh [output-dir]

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

OUT_DIR="${1:-$ROOT/output}"
PTXAS="${PTXAS:-$(command -v ptxas || true)}"

if [[ -z "$PTXAS" ]]; then
    echo "error: ptxas not found; set \$PTXAS or put it on \$PATH" >&2
    exit 1
fi
if [[ ! -d "$OUT_DIR" ]]; then
    echo "error: $OUT_DIR is not a directory" >&2
    exit 1
fi

REPRO="$ROOT/target/release/ptx-fuzz-repro"
if [[ ! -x "$REPRO" ]]; then
    cargo build --release -p ptx-fuzz-repro
fi

TRIAGE="$OUT_DIR/triage"
rm -rf "$TRIAGE"
mkdir -p "$TRIAGE"

# Find every crash file across worker dirs. Skip the README.txt that
# AFL drops into each crashes/ dir.
mapfile -t crashes < <(find "$OUT_DIR" -path "*/crashes/id:*" -type f 2>/dev/null | sort)

if (( ${#crashes[@]} == 0 )); then
    echo "no crashes found in $OUT_DIR"
    exit 0
fi

echo "found ${#crashes[@]} crash file(s); classifying..."

GROUPS_DIR="$TRIAGE/.groups"
mkdir -p "$GROUPS_DIR"

for raw in "${crashes[@]}"; do
    ptx=$("$REPRO" "$raw")
    tmp=$(mktemp --suffix=.ptx)
    printf '%s' "$ptx" > "$tmp"

    set +e
    err=$("$PTXAS" "$tmp" 2>&1 >/dev/null)
    rc=$?
    set -e
    rm -f "$tmp"

    # Signature: exit code + last interesting line. ptxas writes
    # things like "ptxas fatal   : Internal error", "Aborted (core
    # dumped)", or specific error messages. We canonicalize by
    # stripping the tempfile path, which differs every run.
    # grep returning no match would trip pipefail; tolerate that.
    sig_line=$(printf '%s\n' "$err" \
        | { grep -E '(fatal|error|Aborted|Segmentation|Internal)' || true; } \
        | tail -1 \
        | sed 's#/tmp/[^ ,:]*##g')
    sig="${rc}|${sig_line}"
    sig_hash=$(printf '%s' "$sig" | sha1sum | cut -d' ' -f1 | cut -c1-12)

    group_dir="$GROUPS_DIR/$sig_hash"
    mkdir -p "$group_dir"
    printf '%s\n' "$raw" >> "$group_dir/members.txt"
    if [[ ! -f "$group_dir/sig" ]]; then
        printf '%s\n' "$sig" > "$group_dir/sig"
        cp "$raw" "$group_dir/example.bytes"
        printf '%s' "$ptx" > "$group_dir/example.ptx"
        printf '%s' "$err" > "$group_dir/example.stderr"
    fi
done

# Renumber groups deterministically by size (largest first).
{
    echo "# ptx-fuzz triage summary"
    echo "# total crash files: ${#crashes[@]}"
    echo "# groups: $(find "$GROUPS_DIR" -mindepth 1 -maxdepth 1 -type d | wc -l)"
    echo
} > "$TRIAGE/summary.txt"

idx=0
for g in $(find "$GROUPS_DIR" -mindepth 1 -maxdepth 1 -type d \
              -printf '%p ' | xargs -n1 \
              | while read -r d; do echo "$(wc -l < "$d/members.txt") $d"; done \
              | sort -rn | awk '{print $2}'); do
    count=$(wc -l < "$g/members.txt")
    sig=$(cat "$g/sig")
    dst="$TRIAGE/$(printf 'group-%02d' "$idx")"
    mv "$g" "$dst"
    printf 'group-%02d  count=%d  %s\n' "$idx" "$count" "$sig" \
        >> "$TRIAGE/summary.txt"
    idx=$(( idx + 1 ))
done
rm -rf "$GROUPS_DIR"

cat "$TRIAGE/summary.txt"
echo "details: $TRIAGE/group-NN/{example.bytes,example.ptx,example.stderr,members.txt}"
