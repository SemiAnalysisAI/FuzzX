; Reproduces NaN-sign flip in performFNegCombine FCANONICALIZE arm
; (AMDGPUISelLowering.cpp:5402-5427).  Sibling of m107/m110/m111/m120/m127/m128.
;
; The arm folds:
;   (B1)  fneg(fcanonicalize x)         -> fcanonicalize(fneg x)
;   (B2)  fneg(fcanonicalize(fneg x))   -> fcanonicalize x   (double negation
;                                                             collapse)
;
; No `nnan` gate.  `fcanonicalize` is in `fnegFoldsIntoOpcode` at line 684.
;
; On gfx950 HW, `fcanonicalize` lowers to `v_max_f32 x, x`.  AMDGPU HW
; canonicalization of any NaN returns a **positive** qNaN, regardless of
; the input NaN's sign bit:
;
;   v_max_f32(-qNaN, -qNaN) -> +qNaN
;   v_max_f32(+qNaN, +qNaN) -> +qNaN
;
; IR semantics:
;   fneg(fcanonicalize(-qNaN)) = fneg(+qNaN) = -qNaN
;
; The folded form `fcanonicalize(fneg x)` lowers to `v_max -x, -x` and
; produces +qNaN regardless of input sign.  Result diverges.
;
; Test: x = -qNaN (0xFFC00000).
;   Expected (O0, canonical lowering): -qNaN (0xFFC00000)
;   Observed (O2, fold fires):          +qNaN (0x7FC00000)
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m129-performfnegcombine-fcanonicalize-flips-nan-sign/reduced.ll

source_filename = "m129-performfnegcombine-fcanonicalize-flips-nan-sign"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

declare float @llvm.canonicalize.f32(float)
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

  %c = call float @llvm.canonicalize.f32(float %x)
  %n = fsub float -0.0, %c           ; canonical fneg

  %nbits = bitcast float %n to i32

  %o0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %nbits, ptr addrspace(1) %o0
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

; RUN-INPUTS: 0xffc00000
; (x = -qNaN; expected n = -qNaN = 0xFFC00000;
;  observed n at O2 = +qNaN = 0x7FC00000)

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
