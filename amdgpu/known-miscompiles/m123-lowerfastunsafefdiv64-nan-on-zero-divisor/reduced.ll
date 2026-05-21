; Reproduces NaN-on-zero-divisor miscompile in lowerFastUnsafeFDIV64
; (SIISelLowering.cpp:13140-13178).
;
; `fdiv afn double X, Y` with runtime Y=0 lowers to RCP + 4-step NR
; refinement.  RCP(0) = +/-Inf, then:
;
;   Tmp0 = FMA(-Y, R, 1.0)
;        = FMA(-0, +Inf, 1.0)
;        = (-0 * +Inf) + 1.0
;        = NaN + 1.0
;        = NaN
;
;   R = FMA(Tmp0, R, R) = NaN, propagates through final mul.
;
; IEEE/AMDGCN-RCP say `X/+0 = sign(X)*Inf`.  LangRef `afn`
; ("approximate functions") allows imprecision but does NOT permit
; NaN substitution for what IEEE says is +/-Inf -- that would need
; `ninf` + `nnan`.
;
; The f32 fast path (lowerFastUnsafeFDIV line 13136) uses a simple
; `x * RCP(y)` and is safe (`X * +Inf = +/-Inf`).  Only the afn-f64
; path is affected, including the `-1.0/Y` and `1.0/Y` sub-paths
; (13153-13174).
;
; Both -O0 and -O2 emit the SAME buggy asm (same lowering, no
; combine-driven divergence), so the FuzzX O0-vs-O2 oracle does NOT
; catch this.  Witness needs interpreter or `afn` vs non-`afn`
; cross-check.
;
; Test value: x = 1.0, y = 0.0.
;   Expected (IEEE / non-afn): +Inf = 0x7FF0000000000000
;   Observed (afn NR chain):   NaN  = 0x7FF8000000000000 (typical qNaN)
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m123-lowerfastunsafefdiv64-nan-on-zero-divisor/reduced.ll

source_filename = "m123-lowerfastunsafefdiv64-nan-on-zero-divisor"
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
  ; Volatile-load y bits so SDAG can't constant-fold the divide.
  %p0 = getelementptr i32, ptr addrspace(1) %in, i64 0
  %ylo = load volatile i32, ptr addrspace(1) %p0
  %p1 = getelementptr i32, ptr addrspace(1) %in, i64 1
  %yhi = load volatile i32, ptr addrspace(1) %p1
  %ylo64 = zext i32 %ylo to i64
  %yhi64 = zext i32 %yhi to i64
  %yhi64s = shl i64 %yhi64, 32
  %ybits = or i64 %yhi64s, %ylo64
  %y = bitcast i64 %ybits to double

  %r = fdiv afn double 1.0, %y    ; afn permits RCP-only / NR-only paths

  %rbits = bitcast double %r to i64
  %rlo = trunc i64 %rbits to i32
  %rhi64 = lshr i64 %rbits, 32
  %rhi = trunc i64 %rhi64 to i32

  %o0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %rlo, ptr addrspace(1) %o0
  %o1 = getelementptr i32, ptr addrspace(1) %out, i64 1
  store i32 %rhi, ptr addrspace(1) %o1
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

; RUN-INPUTS: 0x00000000, 0x00000000
; (y = +0.0; expected r = +Inf = 0x7FF0000000000000;
;  observed r = NaN = 0x7FF8000000000000)

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
