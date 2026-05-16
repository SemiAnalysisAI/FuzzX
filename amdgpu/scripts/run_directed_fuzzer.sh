#!/usr/bin/env bash
# Run the C++ coverage-guided, GPU-executing AMDGPU differential fuzzer.

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

USER_NAME="${USER:-$(id -u)}"
RUNTIME_ROOT="${FUZZX_RUNTIME_ROOT:-${TMPDIR:-/tmp}/fuzzx-amdgpu-$USER_NAME}"
FUZZER_BIN="${FUZZER_BIN:-$ROOT/build/fuzzer/llvm_amdgpu_diff_fuzzer}"
CORPUS_DIR="${CORPUS_DIR:-$RUNTIME_ROOT/corpus/directed}"
ARTIFACT_DIR="${ARTIFACT_DIR:-$RUNTIME_ROOT/artifacts/directed}"
FUZZX_FINDINGS_DIR="${FUZZX_FINDINGS_DIR:-$RUNTIME_ROOT/findings}"
TMPDIR="${FUZZX_TMPDIR:-$RUNTIME_ROOT/tmp}"
FUZZX_LOCALIZE_FUZZER="${FUZZX_LOCALIZE_FUZZER:-1}"
FUZZX_CPUSET="${FUZZX_CPUSET:-auto}"
ASAN_OPTIONS="${ASAN_OPTIONS:-detect_leaks=0}"

resolve_cpuset() {
    case "$FUZZX_CPUSET" in
        "" | none | off | false | 0)
            return 0
            ;;
        auto)
            if ! command -v taskset >/dev/null || ! command -v python3 >/dev/null; then
                return 0
            fi
            python3 - <<'PY'
import pathlib
import re

try:
    import os
    total = os.cpu_count()
except Exception:
    total = None
if not total:
    raise SystemExit

exclude = set()
for status in pathlib.Path("/proc").glob("[0-9]*/status"):
    proc = status.parent
    try:
        cmdline = (proc / "cmdline").read_bytes().replace(b"\0", b" ").decode(errors="ignore")
        if "wekanode" not in cmdline:
            continue
        text = status.read_text(errors="ignore")
        match = re.search(r"^Cpus_allowed_list:\s*([0-9]+)\s*$", text, re.M)
        if match:
            exclude.add(int(match.group(1)))
    except OSError:
        continue
cpus = [str(cpu) for cpu in range(total) if cpu not in exclude]
if cpus and len(cpus) < total:
    print(",".join(cpus))
PY
            ;;
        *)
            printf '%s\n' "$FUZZX_CPUSET"
            ;;
    esac
}

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

RESOLVED_CPUSET="$(resolve_cpuset)"
CPUSET_CMD=()
if [[ -n "$RESOLVED_CPUSET" ]]; then
    CPUSET_CMD=(taskset -c "$RESOLVED_CPUSET")
fi
export TMPDIR
export FUZZX_FINDINGS_DIR
export ASAN_OPTIONS

exec "${CPUSET_CMD[@]}" "$FUZZER_BIN" "$CORPUS_DIR" \
    -artifact_prefix="$ARTIFACT_DIR/" \
    "$@"
