#!/usr/bin/env bash
# Compile a single AMDGPU LLVM IR reproducer at -O0 and -O2, run both through
# HIP, and print the observed output words.

set -euo pipefail

CALLER_PWD="$(pwd)"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ "${1:-}" == /* ]]; then
    LL_FILE="$1"
elif [[ -n "${1:-}" ]]; then
    LL_FILE="$CALLER_PWD/$1"
else
    LL_FILE="$ROOT/known-miscompiles/m001-ashr-i16-zext/reduced.ll"
fi
INPUTS_TEXT="${2:-}"
DEVICE="${3:-0}"

ROCM_PATH="${ROCM_PATH:-/opt/rocm-7.1.1}"
MCPU="${MCPU:-gfx950}"
CLANG="${CLANG:-$ROCM_PATH/lib/llvm/bin/clang}"
LLD="${LLD:-$ROCM_PATH/lib/llvm/bin/lld}"
HIPCC="${HIPCC:-$ROCM_PATH/bin/hipcc}"
RUNNER="${RUNNER:-$ROOT/build/hip_module_runner}"

cd "$ROOT"

if [[ ! -f "$LL_FILE" ]]; then
    echo "LLVM IR file not found: $LL_FILE" >&2
    exit 2
fi

if [[ ! -x "$CLANG" ]]; then
    echo "clang not found or not executable: $CLANG" >&2
    exit 2
fi

if [[ ! -x "$LLD" ]]; then
    echo "lld not found or not executable: $LLD" >&2
    exit 2
fi

if [[ ! -x "$RUNNER" ]]; then
    mkdir -p "$ROOT/build"
    "$HIPCC" -O2 "$ROOT/runner/hip_module_runner.cpp" -o "$RUNNER"
fi

if [[ -z "$INPUTS_TEXT" ]]; then
    INPUTS_TEXT="$(sed -n -E 's/^[[:space:]]*;[[:space:]]*RUN-INPUTS:[[:space:]]*//p' "$LL_FILE" | head -n 1)"
fi

if [[ -z "$INPUTS_TEXT" ]]; then
    echo "no input values specified" >&2
    echo "pass inputs as the second argument, or add '; RUN-INPUTS: 0x...' to the .ll file" >&2
    exit 2
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

INPUT_COUNT="$(python3 - "$INPUTS_TEXT" "$TMPDIR/input.bin" <<'PY'
import re
import struct
import sys

text = sys.argv[1].strip().strip("[]")
tokens = [token for token in re.split(r"[\s,]+", text) if token]
values = [int(token, 0) & 0xffffffff for token in tokens]
if not values:
    raise SystemExit("no input values parsed")
with open(sys.argv[2], "wb") as f:
    for value in values:
        f.write(struct.pack("<I", value))
print(len(values))
PY
)"

for opt in O0 O2; do
    "$CLANG" "-$opt" -nogpulib -target amdgcn-amd-amdhsa -mcpu="$MCPU" \
        -x ir -c "$LL_FILE" -o "$TMPDIR/$opt.o"
    "$LLD" -flavor gnu -shared "$TMPDIR/$opt.o" -o "$TMPDIR/$opt.hsaco"
    "$RUNNER" "$TMPDIR/$opt.hsaco" "$TMPDIR/input.bin" "$TMPDIR/$opt.out" \
        "$INPUT_COUNT" "$DEVICE" "$INPUT_COUNT"
done

python3 - "$INPUTS_TEXT" "$TMPDIR/O0.out" "$TMPDIR/O2.out" <<'PY'
import re
import struct
import sys

text = sys.argv[1].strip().strip("[]")
inputs = [int(token, 0) & 0xffffffff
          for token in re.split(r"[\s,]+", text) if token]

def read_u32s(path):
    with open(path, "rb") as f:
        data = f.read()
    return list(struct.unpack("<" + "I" * (len(data) // 4), data))

o0_values = read_u32s(sys.argv[2])
o2_values = read_u32s(sys.argv[3])

if len(inputs) == 1:
    print(f"input=0x{inputs[0]:08x}")
    print(f"O0=0x{o0_values[0]:08x}")
    print(f"O2=0x{o2_values[0]:08x}")
    print(f"mismatch={'true' if o0_values[0] != o2_values[0] else 'false'}")
else:
    any_mismatch = False
    for index, (input_value, o0, o2) in enumerate(zip(inputs, o0_values, o2_values)):
        mismatch = o0 != o2
        any_mismatch |= mismatch
        print(f"[{index}] input=0x{input_value:08x} O0=0x{o0:08x} O2=0x{o2:08x} mismatch={'true' if mismatch else 'false'}")
    print(f"any_mismatch={'true' if any_mismatch else 'false'}")
PY
