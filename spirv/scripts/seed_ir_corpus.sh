#!/usr/bin/env bash
# Seed an empty SPIR-V fuzzer corpus with a valid LLVM bitcode module
# targeting spirv64.  Mirrors amdgpu/scripts/seed_ir_corpus.sh.

set -euo pipefail

ROOT="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.." && pwd)"

if [[ "$#" -ne 1 ]]; then
    echo "usage: $0 CORPUS_DIR" >&2
    exit 2
fi

CORPUS_DIR="$1"
mkdir -p "$CORPUS_DIR"
if compgen -G "$CORPUS_DIR/*" >/dev/null; then
    exit 0
fi

find_opt() {
    if [[ -n "${LLVM_OPT:-}" ]]; then
        printf '%s\n' "$LLVM_OPT"
        return 0
    fi

    local candidate
    for candidate in \
        "$ROOT/build/llvm-fuzzer/bin/opt" \
        "$ROOT/build/llvm-fuzzer-install/bin/opt" \
        opt; do
        if [[ "$candidate" == */* ]]; then
            if [[ -x "$candidate" ]]; then
                printf '%s\n' "$candidate"
                return 0
            fi
        elif command -v "$candidate" >/dev/null 2>&1; then
            command -v "$candidate"
            return 0
        fi
    done
    return 1
}

LLVM_OPT_BIN="$(find_opt)" || {
    echo "could not find LLVM opt; set LLVM_OPT=/path/to/opt" >&2
    exit 2
}

TMP_LL="$CORPUS_DIR/.seed-$$.ll"
TMP_BC="$CORPUS_DIR/.seed-$$.bc"
trap 'rm -f "$TMP_LL" "$TMP_BC"' EXIT

cat >"$TMP_LL" <<'EOF'
target triple = "spirv64-unknown-unknown"

define spir_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) {
entry:
  %ok = icmp sgt i32 %n, 0
  br i1 %ok, label %body, label %exit

body:
  %v = load i32, ptr addrspace(1) %in, align 4
  %salt = mul i32 %n, -1640531527
  %mix = xor i32 %v, %salt
  store i32 %mix, ptr addrspace(1) %out, align 4
  br label %exit

exit:
  ret void
}
EOF

"$LLVM_OPT_BIN" -o "$TMP_BC" "$TMP_LL"
mv "$TMP_BC" "$CORPUS_DIR/seed.bc"
