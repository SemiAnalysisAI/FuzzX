#!/usr/bin/env bash
# Compile a single AMDGPU LLVM IR reproducer at -O0 and -O2, run both through
# HIP, and print the observed output words.

set -euo pipefail

CALLER_PWD="$(pwd)"
SCRIPT_DIR="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
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
REPEAT_TEXT="${4:-}"

ROCM_PATH="${ROCM_PATH:-/opt/rocm-7.1.1}"
MCPU="${MCPU:-gfx950}"
HIPCC="${HIPCC:-$ROCM_PATH/bin/hipcc}"
RUNNER="${RUNNER:-$ROOT/build/hip_module_runner}"

cd "$ROOT"

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
    if [[ -z "${LLD+x}" ]]; then
        LLD="$RUN_LLVM_BUILD_DIR/bin/lld"
    fi
fi

CLANG="${CLANG:-$ROCM_PATH/lib/llvm/bin/clang}"
LLD="${LLD:-$ROCM_PATH/lib/llvm/bin/lld}"

if [[ ! -x "$CLANG" ]]; then
    echo "clang not found or not executable: $CLANG" >&2
    exit 2
fi

if [[ ! -x "$LLD" ]]; then
    echo "lld not found or not executable: $LLD" >&2
    exit 2
fi

if [[ ! -x "$RUNNER" || "$ROOT/runner/hip_module_runner.cpp" -nt "$RUNNER" ]]; then
    mkdir -p "$ROOT/build"
    "$HIPCC" -O2 "$ROOT/runner/hip_module_runner.cpp" -o "$RUNNER"
fi

if [[ -z "$INPUTS_TEXT" ]]; then
    INPUTS_TEXT="$(sed -n -E 's/^[[:space:]]*;[[:space:]]*RUN-INPUTS:[[:space:]]*//p' "$LL_FILE" | head -n 1)"
fi

if [[ -z "$REPEAT_TEXT" ]]; then
    REPEAT_TEXT="$(sed -n -E 's/^[[:space:]]*;[[:space:]]*RUN-REPEAT:[[:space:]]*//p' "$LL_FILE" | head -n 1)"
fi

RUN_COMBINED_TEXT="$(sed -n -E 's/^[[:space:]]*;[[:space:]]*RUN-COMBINED:[[:space:]]*//p' "$LL_FILE" | head -n 1)"
RUN_COMBINED=0
if [[ "$RUN_COMBINED_TEXT" =~ ^(1|true|TRUE|yes|YES|on|ON)$ ]]; then
    RUN_COMBINED=1
fi

REPEAT="${REPEAT_TEXT:-1}"

if [[ -z "$INPUTS_TEXT" ]]; then
    echo "no input values specified" >&2
    echo "pass inputs as the second argument, or add '; RUN-INPUTS: 0x...' to the .ll file" >&2
    exit 2
fi

if ! [[ "$REPEAT" =~ ^[1-9][0-9]*$ ]]; then
    echo "RUN-REPEAT must be a positive integer: $REPEAT" >&2
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
values = []
for token in tokens:
    if "*" in token:
        value_text, count_text = token.rsplit("*", 1)
        count = int(count_text, 0)
        if count < 0:
            raise SystemExit(f"negative repeat count in {token!r}")
        values.extend([int(value_text, 0) & 0xffffffff] * count)
    else:
        values.append(int(token, 0) & 0xffffffff)
if not values:
    raise SystemExit("no input values parsed")
with open(sys.argv[2], "wb") as f:
    for value in values:
        f.write(struct.pack("<I", value))
print(len(values))
PY
)"

if [[ "$RUN_COMBINED" -eq 1 ]]; then
    python3 - "$LL_FILE" "$TMPDIR/O0.ll" "$TMPDIR/O2.ll" <<'PY'
import sys

text = open(sys.argv[1], encoding="utf-8").read()
for path, name in [(sys.argv[2], "fuzz_kernel_o0"), (sys.argv[3], "fuzz_kernel_o2")]:
    with open(path, "w", encoding="utf-8") as f:
        f.write(text.replace("fuzz_kernel", name))
PY
    "$CLANG" -O0 -nogpulib -target amdgcn-amd-amdhsa -mcpu="$MCPU" \
        -x ir -c "$TMPDIR/O0.ll" -o "$TMPDIR/O0.o"
    "$CLANG" -O2 -nogpulib -target amdgcn-amd-amdhsa -mcpu="$MCPU" \
        -x ir -c "$TMPDIR/O2.ll" -o "$TMPDIR/O2.o"
    "$LLD" -flavor gnu -shared "$TMPDIR/O0.o" "$TMPDIR/O2.o" \
        -o "$TMPDIR/combined.hsaco"
else
    for opt in O0 O2; do
        "$CLANG" "-$opt" -nogpulib -target amdgcn-amd-amdhsa -mcpu="$MCPU" \
            -x ir -c "$LL_FILE" -o "$TMPDIR/$opt.o"
        "$LLD" -flavor gnu -shared "$TMPDIR/$opt.o" -o "$TMPDIR/$opt.hsaco"
    done
fi

for ((iteration = 1; iteration <= REPEAT; ++iteration)); do
    if [[ "$RUN_COMBINED" -eq 1 ]]; then
        "$RUNNER" "$TMPDIR/combined.hsaco" "$TMPDIR/input.bin" "$TMPDIR/O0.out" \
            "$INPUT_COUNT" "$DEVICE" "$INPUT_COUNT" fuzz_kernel_o0
        "$RUNNER" "$TMPDIR/combined.hsaco" "$TMPDIR/input.bin" "$TMPDIR/O2.out" \
            "$INPUT_COUNT" "$DEVICE" "$INPUT_COUNT" fuzz_kernel_o2
    else
        for opt in O0 O2; do
            "$RUNNER" "$TMPDIR/$opt.hsaco" "$TMPDIR/input.bin" "$TMPDIR/$opt.out" \
                "$INPUT_COUNT" "$DEVICE" "$INPUT_COUNT"
        done
    fi

    if [[ "$REPEAT" -eq 1 ]]; then
        python3 - "$INPUTS_TEXT" "$TMPDIR/O0.out" "$TMPDIR/O2.out" full "$iteration" <<'PY'
import re
import struct
import sys

text = sys.argv[1].strip().strip("[]")
tokens = [token for token in re.split(r"[\s,]+", text) if token]
inputs = []
for token in tokens:
    if "*" in token:
        value_text, count_text = token.rsplit("*", 1)
        inputs.extend([int(value_text, 0) & 0xffffffff] * int(count_text, 0))
    else:
        inputs.append(int(token, 0) & 0xffffffff)

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
    else
        if python3 - "$INPUTS_TEXT" "$TMPDIR/O0.out" "$TMPDIR/O2.out" brief "$iteration" <<'PY'
import re
import struct
import sys

text = sys.argv[1].strip().strip("[]")
tokens = [token for token in re.split(r"[\s,]+", text) if token]
inputs = []
for token in tokens:
    if "*" in token:
        value_text, count_text = token.rsplit("*", 1)
        inputs.extend([int(value_text, 0) & 0xffffffff] * int(count_text, 0))
    else:
        inputs.append(int(token, 0) & 0xffffffff)

def read_u32s(path):
    with open(path, "rb") as f:
        data = f.read()
    return list(struct.unpack("<" + "I" * (len(data) // 4), data))

o0_values = read_u32s(sys.argv[2])
o2_values = read_u32s(sys.argv[3])
iteration = int(sys.argv[5])

for index, (input_value, o0, o2) in enumerate(zip(inputs, o0_values, o2_values)):
    if o0 != o2:
        print(f"iteration={iteration}")
        print(f"index={index}")
        print(f"input=0x{input_value:08x}")
        print(f"O0=0x{o0:08x}")
        print(f"O2=0x{o2:08x}")
        print("mismatch=true")
        raise SystemExit(0)
raise SystemExit(1)
PY
        then
            exit 0
        fi
    fi
done

if [[ "$REPEAT" -gt 1 ]]; then
    echo "mismatch=false"
    echo "iterations=$REPEAT"
fi
