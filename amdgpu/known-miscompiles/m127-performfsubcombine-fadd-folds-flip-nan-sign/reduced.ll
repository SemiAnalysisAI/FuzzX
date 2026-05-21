; Reproduces NaN-sign-flip miscompile in performFSubCombine two arms
; (SIISelLowering.cpp:17579-17624).  Sibling of m107.
;
; Two arms, both gated only on `getFusedOpcode != 0` (which requires
; `contract` on both ops) -- neither checks `nnan`:
;
;   Arm 1 (17596-17608): (fsub (fadd a,a), c) -> fma(a, 2.0, fneg(c))
;   Arm 2 (17610-17621): (fsub c, (fadd a,a)) -> fma(a, -2.0, c)
;
; For non-NaN finite operands both rewrites are bit-exact.  For NaN
; operands, the HW v_sub's implicit NEG-on-b DOES flip the propagated
; NaN's sign, while the VOP3 NEG src-modifier on v_fma_f32 does NOT
; (HW NaN-propagation rule, same as m107).
;
; Arm 1 (this kernel):
;   a = 1.0, c = +qNaN (0x7FC00000)
;   Expected (O0): -qNaN (0xFFC00000)
;   Observed (O2):  +qNaN (0x7FC00000)
;
; Same shape as m107 (FMul NaN sign) but in performFSubCombine instead
; of performFNegCombine.
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m127-performfsubcombine-fadd-folds-flip-nan-sign/reduced.ll

source_filename = "m127-performfsubcombine-fadd-folds-flip-nan-sign"
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
  %p0 = getelementptr i32, ptr addrspace(1) %in, i64 0
  %ai = load volatile i32, ptr addrspace(1) %p0
  %p1 = getelementptr i32, ptr addrspace(1) %in, i64 1
  %ci = load volatile i32, ptr addrspace(1) %p1
  %a = bitcast i32 %ai to float
  %c = bitcast i32 %ci to float

  %sum = fadd contract float %a, %a       ; 2a
  %r   = fsub contract float %sum, %c     ; Arm 1: -> fma(a, 2.0, fneg(c))

  %rbits = bitcast float %r to i32

  %o0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %rbits, ptr addrspace(1) %o0
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

; RUN-INPUTS: 0x3f800000, 0x7fc00000
; (a = 1.0; c = +qNaN; expected r = -qNaN = 0xFFC00000;
;  observed r = +qNaN = 0x7FC00000)

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
