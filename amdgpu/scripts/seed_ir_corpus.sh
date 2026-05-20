#!/usr/bin/env bash
# Seed an empty AMDGPU fuzzer corpus with a valid LLVM bitcode module.

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
        "$ROOT/build/llvm-rocm-7.2.3-cov-release/bin/opt" \
        "$ROOT/build/llvm-instrumented/bin/opt" \
        /opt/rocm/lib/llvm/bin/opt \
        /opt/rocm-7.2.3/lib/llvm/bin/opt \
        /opt/rocm-7.1.1/lib/llvm/bin/opt \
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

MCPU="${AMDGPU_MCPU:-gfx950}"
TMP_LL="$CORPUS_DIR/.seed-$$.ll"
TMP_BC="$CORPUS_DIR/.seed-$$.bc"
trap 'rm -f "$TMP_LL" "$TMP_BC"' EXIT

cat >"$TMP_LL" <<EOF
target datalayout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wg = call i32 @llvm.amdgcn.workgroup.id.x()
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %block.base = mul i32 %wg, 256
  %idx = add i32 %block.base, %wi
  %ok = icmp ult i32 %idx, %n
  br i1 %ok, label %body, label %exit

body:
  %idx64 = zext i32 %idx to i64
  %in.ptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %v = load i32, ptr addrspace(1) %in.ptr, align 4
  %salt = mul i32 %idx, -1640531527
  %mix = xor i32 %v, %salt
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %mix, ptr addrspace(1) %out.ptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workgroup.id.x()
declare i32 @llvm.amdgcn.workitem.id.x()

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="$MCPU" "uniform-work-group-size"="true" }

!llvm.module.flags = !{!0, !1, !2}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
!1 = !{i32 1, !"amdgpu_printf_kind", !"hostcall"}
!2 = !{i32 8, !"PIC Level", i32 2}
EOF

"$LLVM_OPT_BIN" -o "$TMP_BC" "$TMP_LL"
mv "$TMP_BC" "$CORPUS_DIR/seed.bc"
