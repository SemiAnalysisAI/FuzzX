#!/usr/bin/env bash
# Compile a single AMDGPU LLVM IR reproducer at -O0 and -O2 and print whether
# either compiler invocation fails.

set -u

CALLER_PWD="$(pwd)"
SCRIPT_DIR="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ "${1:-}" == /* ]]; then
    LL_FILE="$1"
elif [[ -n "${1:-}" ]]; then
    LL_FILE="$CALLER_PWD/$1"
else
    LL_FILE="$ROOT/known-miscompiles/c001-sudot-isel-ice/reduced-sudot8.ll"
fi

ROCM_PATH="${ROCM_PATH:-/opt/rocm-7.1.1}"

if [[ ! -f "$LL_FILE" ]]; then
    echo "LLVM IR file not found: $LL_FILE" >&2
    exit 2
fi

RUN_LLVM_BUILD="$(sed -n -E 's/^[[:space:]]*;[[:space:]]*RUN-LLVM-BUILD:[[:space:]]*//p' "$LL_FILE" | head -n 1)"
if [[ -n "$RUN_LLVM_BUILD" ]]; then
    if [[ "$RUN_LLVM_BUILD" == /* ]]; then
        RUN_LLVM_BUILD_DIR="$RUN_LLVM_BUILD"
    else
        RUN_LLVM_BUILD_DIR="$ROOT/$RUN_LLVM_BUILD"
    fi
    if [[ -z "${CLANG+x}" ]]; then
        CLANG="$RUN_LLVM_BUILD_DIR/bin/clang"
    fi
fi

CLANG="${CLANG:-$ROCM_PATH/lib/llvm/bin/clang}"
MCPU="${MCPU:-$(sed -n -E 's/^[[:space:]]*;[[:space:]]*RUN-MCPU:[[:space:]]*//p' "$LL_FILE" | head -n 1)}"
MCPU="${MCPU:-gfx950}"

if [[ ! -x "$CLANG" ]]; then
    echo "clang not found or not executable: $CLANG" >&2
    exit 2
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

ANY_FAILURE=0
for opt in O0 O2; do
    LOG="$TMPDIR/$opt.log"
    "$CLANG" "-$opt" -nogpulib -target amdgcn-amd-amdhsa -mcpu="$MCPU" \
        -x ir -c "$LL_FILE" -o "$TMPDIR/$opt.o" >"$LOG" 2>&1
    RC="$?"
    if [[ "$RC" -eq 0 ]]; then
        echo "$opt=pass"
        continue
    fi

    ANY_FAILURE=1
    echo "$opt=fail"
    echo "$opt-exit=$RC"
    SUMMARY="$(grep -m1 -E 'Cannot select|fatal error|LLVM ERROR|PLEASE submit' "$LOG" || true)"
    if [[ -n "$SUMMARY" ]]; then
        echo "$opt-message=$SUMMARY"
    fi
done

if [[ "$ANY_FAILURE" -eq 1 ]]; then
    echo "compiler_failure=true"
else
    echo "compiler_failure=false"
fi
