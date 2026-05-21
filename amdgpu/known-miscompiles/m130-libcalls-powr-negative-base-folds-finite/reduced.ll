; Reproduces OCL `powr` spec violation in AMDGPULibCalls::fold_pow
; (AMDGPULibCalls.cpp:900-1005).  Sibling of m093 (which covers `pow`,
; not `powr`).
;
; OpenCL/IEEE `powr(x, y)`:
;
;   powr(x, y) is NaN whenever x < 0 (the base MUST be >= 0)
;   powr(NaN, 0) = NaN  (powr explicitly diverges from C `pow`)
;   powr(+0, 0)  = NaN
;   powr(-0, 0)  = NaN
;
; The constant-exponent shortcuts in `fold_pow` (lines 900-934 and the
; unsafe-math integer expansion at 971-1005) do NOT check
; `FInfo.getId()`, so they fire for `EI_POWR` / `EI_POWR_FAST` too:
;
;   powr(x, 0)  -> 1.0          (wrong: should be NaN when x is NaN or 0)
;   powr(x, 1)  -> x
;   powr(x, 2)  -> x*x          (wrong: should be NaN when x < 0)
;   powr(x, -1) -> 1.0/x        (wrong: should be NaN when x < 0)
;   powr(x, ±0.5) -> sqrt/rsqrt (m093 covers this for `pow`; same here for `powr`)
;
; Test value: y = 2.0, x is loaded as runtime input.  For x = -2.0:
;   Expected (IEEE / non-folded): NaN
;   Observed (fold fires):        4.0 = 0x40800000
;
; Companion of m093 (`pow(x, ±0.5)` without nnan/ninf).
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m130-libcalls-powr-negative-base-folds-finite/reduced.ll

source_filename = "m130-libcalls-powr-negative-base-folds-finite"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

; In-module powr declaration so AMDGPULibCalls fires.
declare protected float @_Z4powrff(float, float)

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
  %p0 = getelementptr i32, ptr addrspace(1) %in, i64 0
  %xi = load volatile i32, ptr addrspace(1) %p0
  %x  = bitcast i32 %xi to float

  %r = call float @_Z4powrff(float %x, float 2.0)

  %rbits = bitcast float %r to i32

  %o0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %rbits, ptr addrspace(1) %o0
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

; RUN-INPUTS: 0xc0000000
; (x = -2.0; expected r = NaN per OCL powr; observed r = 4.0 = 0x40800000)

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
