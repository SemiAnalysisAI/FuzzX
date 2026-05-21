; Reproduces NaN-sign miscompile in performFMulCombine fneg-LHS arm
; (SIISelLowering.cpp:17719-17721):
;
;   fmul x, (select y, -A, -B)  ->  ldexp(fneg x, select(y, log2(A), log2(B)))
;
; When both select arms are negative powers of two, the combine wraps
; LHS in FNEG with no `nnan` guard.  Under AMDGPU HW semantics:
;
;   v_mul_f64(NaN, -K) preserves input NaN's sign bit
;   v_ldexp_f64(-x, k) lowers FNEG into VOP3 NEG src-modifier, which
;     XORs the sign bit BEFORE ldexp sees the operand, then ldexp
;     preserves that flipped sign in its NaN propagation.
;
; So for any NaN-valued x, the transformed code flips the sign relative
; to the original.  Inverse of m107 (m107: original IR flips NaN sign
; that the fold preserves; m120: original IR preserves NaN sign that
; the fold flips).
;
; Test value: x = +qNaN (0x7FF8000000000000), y = true (picks -2.0).
;   Expected (O0): 0x7FF8000000000000 (+qNaN)
;   Observed (O2): 0xFFF8000000000000 (-qNaN)
;
; Affects f64 (always), and f32/f16 in divergent contexts.
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m120-performfmulcombine-fneg-lhs-flips-nan-sign/reduced.ll

source_filename = "m120-performfmulcombine-fneg-lhs-flips-nan-sign"
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
  ; Volatile-load NaN bits + selector so SDAG can't constant-fold.
  %p0 = getelementptr i32, ptr addrspace(1) %in, i64 0
  %xilo = load volatile i32, ptr addrspace(1) %p0
  %p1 = getelementptr i32, ptr addrspace(1) %in, i64 1
  %xihi = load volatile i32, ptr addrspace(1) %p1
  %p2 = getelementptr i32, ptr addrspace(1) %in, i64 2
  %yi = load volatile i32, ptr addrspace(1) %p2

  %xlo64 = zext i32 %xilo to i64
  %xhi64 = zext i32 %xihi to i64
  %xhi64s = shl i64 %xhi64, 32
  %xbits = or i64 %xhi64s, %xlo64
  %x = bitcast i64 %xbits to double
  %y = icmp ne i32 %yi, 0

  %sel = select i1 %y, double -2.0, double -4.0
  %m = fmul double %x, %sel          ; fold fires: v_ldexp_f64 -x, k

  %mbits = bitcast double %m to i64
  %mlo = trunc i64 %mbits to i32
  %mhi64 = lshr i64 %mbits, 32
  %mhi = trunc i64 %mhi64 to i32

  %o0 = getelementptr i32, ptr addrspace(1) %out, i64 0
  store i32 %mlo, ptr addrspace(1) %o0
  %o1 = getelementptr i32, ptr addrspace(1) %out, i64 1
  store i32 %mhi, ptr addrspace(1) %o1
  br label %exit

exit:
  ret void
}

attributes #0 = { nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }

; RUN-INPUTS: 0x00000000, 0x7ff80000, 0x00000001
; (x = +qNaN double = 0x7FF8000000000000;
;  y = 1 -> selects -2.0;
;  expected mhi = 0x7FF80000 (+qNaN);
;  observed mhi = 0xFFF80000 (-qNaN))

!llvm.module.flags = !{!0}
!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
