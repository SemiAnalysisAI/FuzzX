; Reproduces sign-of-zero miscompile in SelectVOP3ModsImpl
; (AMDGPUISelDAGToDAG.cpp:3415-3423):
;
;   fsub +0.0, x   ->   VOP3 NEG src-modifier on x   (i.e., fneg x)
;
; The fold uses `LHS->isZero()` which matches BOTH +0.0 AND -0.0.
; Under IEEE 754, `fsub +0.0, x` differs from `fneg(x)` exactly when
; `x = +0.0`:
;
;   fsub +0.0, +0.0 = +0.0
;   -(+0.0)         = -0.0
;
; The asymmetric case `fsub -0.0, x` IS algebraically equal to `-x` for
; all x, so the right fix is either:
;   (a) restrict to LHS->isNegZero(), or
;   (b) gate on Src->getFlags().hasNoSignedZeros().
;
; Test value: x = +0.0, y = +1.0.
;   Expected (IEEE/GISel): 0x00000000 (+0.0)
;   Observed (SDAG):       0x80000000 (-0.0)
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m106-selectvop3mods-fsub-pzero-sign-of-zero/reduced.ll

source_filename = "m106-selectvop3mods-fsub-pzero-sign-of-zero"
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
  ; Volatile loads of bit-pattern inputs (so the fold can't see constants).
  %p0 = getelementptr i32, ptr addrspace(1) %in, i64 0
  %xi = load volatile i32, ptr addrspace(1) %p0
  %p1 = getelementptr i32, ptr addrspace(1) %in, i64 1
  %yi = load volatile i32, ptr addrspace(1) %p1
  %x  = bitcast i32 %xi to float
  %y  = bitcast i32 %yi to float

  %neg = fsub float 0.0, %x      ; <- fold fires: NEG src-modifier on x
  %r   = fmul float %neg, %y     ; <- VOP3 consumer of NEG

  %rbits = bitcast float %r to i32

  %o0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %rbits, ptr addrspace(1) %o0
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

; RUN-INPUTS: 0x00000000, 0x3f800000
; (x = +0.0, y = +1.0; expected r = +0.0 = 0x00000000; observed r = -0.0 = 0x80000000)

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
