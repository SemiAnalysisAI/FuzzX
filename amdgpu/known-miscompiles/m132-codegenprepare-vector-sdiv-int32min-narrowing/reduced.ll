; Reproduces a per-lane (v2i64) extension of the m103 i64-sdiv
; narrowing miscompile.
;
; AMDGPUCodeGenPrepare.cpp:1488-1520 scalarizes a vector i64 div/rem
; by extracting each lane and calling shrinkDivRem64 (line 1503) on
; each scalar i64 element.  shrinkDivRem64 in turn checks
; getDivNumBits(...) > 32 (line 1354-1356) per lane: if the per-lane
; LHS has ComputeNumSignBits > 32 (e.g. sext(INT32_MIN) = 33 sign
; bits) and the per-lane RHS also has > 32 sign bits (any sext from
; i32, including the splat divisor `<i64 -1, i64 -1>`), shrinkDivRem64
; narrows the lane to an i32 sdiv -- which is i32-overflow / poison
; for `sdiv 0x80000000, -1` and is then SExt-promoted back to i64,
; yielding 0xFFFFFFFF_80000000.  The well-defined i64 result is
; +2^31 = 0x00000000_80000000.
;
; To get a clean O0 vs O2 mismatch we use a *literal* divisor splat
; `<i64 -1, i64 -1>`: O2 InstCombine pre-folds `sdiv x, -1 -> 0 - x`
; on the vector before AMDGPUCodeGenPrepare can see it, so the
; narrowing path doesn't fire and the result is correct.  O0 keeps
; the literal-divisor sdiv intact, AMDGPUCodeGenPrepare scalarizes
; and per-lane narrows, and lane 0 (INT32_MIN-fed) is wrong.
;
; The volatile load of the LHS keeps both ComputeNumSignBits results
; available (sext of an i32 load is 33 sign bits, matching the
; INT32_MIN case in lane 0; lane 1 happens to be sext(100) = 57 sign
; bits, also > 32, and divides correctly either way).
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m132-codegenprepare-vector-sdiv-int32min-narrowing/reduced.ll

source_filename = "m132-codegenprepare-vector-sdiv-int32min-narrowing"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

declare noundef i32 @llvm.amdgcn.workitem.id.x() #1
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %workgroup = call i32 @llvm.amdgcn.workgroup.id.x()
  %workitem  = call i32 @llvm.amdgcn.workitem.id.x()
  %base      = mul i32 %workgroup, 256
  %idx       = add i32 %base, %workitem
  %in.range  = icmp eq i32 %idx, 0
  br i1 %in.range, label %body, label %exit

body:
  ; Lane 0 LHS = sext(loaded INT32_MIN) -- 33 sign bits.
  %p0   = getelementptr i32, ptr addrspace(1) %in, i64 0
  %lo0  = load volatile i32, ptr addrspace(1) %p0
  %lo64 = sext i32 %lo0 to i64

  ; Lane 1 LHS = sext(loaded 100) -- many sign bits, divides cleanly.
  %p1   = getelementptr i32, ptr addrspace(1) %in, i64 1
  %lo1  = load volatile i32, ptr addrspace(1) %p1
  %hi64 = sext i32 %lo1 to i64

  ; Build the v2i64 numerator from the two sext-from-i32 lanes.
  %num0 = insertelement <2 x i64> poison, i64 %lo64, i32 0
  %num  = insertelement <2 x i64> %num0,  i64 %hi64, i32 1

  ; Literal splat divisor <-1, -1>.  At O2 InstCombine folds
  ; `sdiv <2 x i64> %num, splat(-1)` into `0 - %num` (correct).
  ; At O0 the literal sdiv survives into AMDGPUCodeGenPrepare,
  ; which scalarizes and per-lane narrows -- lane 0 hits the
  ; INT32_MIN/-1 overflow case.
  %q = sdiv <2 x i64> %num, <i64 -1, i64 -1>

  ; Extract the high half of lane 0 -- that's where the buggy
  ; SIGN_EXTEND of the narrowed i32 sdiv shows the difference:
  ;   correct: 0x00000000
  ;   buggy:   0xFFFFFFFF
  %q0    = extractelement <2 x i64> %q, i32 0
  %q0hi  = lshr i64 %q0, 32
  %q0hi32 = trunc i64 %q0hi to i32
  %q0lo32 = trunc i64 %q0 to i32

  %o0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %q0lo32, ptr addrspace(1) %o0
  %o1 = getelementptr i32, ptr addrspace(1) %out, i64 1
  store i32 %q0hi32, ptr addrspace(1) %o1
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

; RUN-INPUTS: 0x80000000, 0x00000064
; (lane0 = sext(INT32_MIN) = -2147483648, lane1 = sext(100) = 100.
;  divisor splat = <-1, -1>.
;  True lane-0 quotient = +2147483648 = 0x00000000_80000000.
;  Observed at O0: low32 = 0x80000000, high32 = 0xFFFFFFFF (narrowed).
;  Observed at O2: low32 = 0x80000000, high32 = 0x00000000 (correct).)
; RUN-LLVM-BUILD: build/llvm-fuzzer

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
