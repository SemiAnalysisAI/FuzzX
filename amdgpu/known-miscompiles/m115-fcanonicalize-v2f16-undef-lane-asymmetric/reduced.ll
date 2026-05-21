; Reproduces v2f16 fcanonicalize asymmetric undef-lane fixup in
; SIISelLowering.cpp:15910-15915.
;
; performFCanonicalizeCombine has a build_vector path for v2f16 that
; tries to fix the case where one lane is undef.  The symmetric branch
; at 15917-15921 (for NewElts[1].isUndef()) correctly falls back to
; getConstantFP(0.0).  The other branch at 15910-15915 has a dead
; ternary:
;
;   if (NewElts[0].isUndef()) {
;     if (isa<ConstantFPSDNode>(NewElts[1]))
;       NewElts[0] = isa<ConstantFPSDNode>(NewElts[1])
;                        ? NewElts[1]
;                        : DAG.getConstantFP(0.0f, SL, EltVT);  // unreachable
;   }
;
; The guard means NewElts[0] is only fixed when the OTHER lane is a
; ConstantFPSDNode.  If the other lane is a non-const (e.g.
; FCANONICALIZE of a runtime value), NewElts[0] stays undef and the
; combined build_vector lets the low lane pass through raw register
; bits at O2.  The fcanonicalize(undef) -> qNaN contract (line 15868)
; is defeated.
;
; Asm divergence (gfx950, denormal-fp-math=preserve-sign):
;   O0: s_lshl_b32 s2, s2, 16
;       v_pk_max_f16 v1, s2, s2          (low half = 0x0000 from lshl)
;   O2: v_mov_b32_e32 v1, s2
;       v_max_f16_sdwa v1, s2, v1 dst_sel:WORD_1   (low half = raw bits of s2)
;
; With sNaN or denormal junk in the low 16 bits of s2, the O2 lane is
; observably non-canonical.
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m115-fcanonicalize-v2f16-undef-lane-asymmetric/reduced.ll

source_filename = "m115-fcanonicalize-v2f16-undef-lane-asymmetric"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

declare <2 x half> @llvm.canonicalize.v2f16(<2 x half>)
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
  %xh = trunc i32 %xi to i16
  %x  = bitcast i16 %xh to half

  ; Build v2f16 = (poison, x) -- low lane is undef, high lane is runtime.
  %iv = insertelement <2 x half> poison, half %x, i32 1
  %cv = call <2 x half> @llvm.canonicalize.v2f16(<2 x half> %iv)
  %bc = bitcast <2 x half> %cv to i32

  %o0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %bc, ptr addrspace(1) %o0
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "denormal-fp-math"="preserve-sign,preserve-sign" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

; RUN-INPUTS: 0x7e017c01
; (x kernarg low 16 bits as a sNaN-flushed value; high 16 bits = junk
;  that should NOT survive canonicalize on the low half; O2 leaks them)

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
