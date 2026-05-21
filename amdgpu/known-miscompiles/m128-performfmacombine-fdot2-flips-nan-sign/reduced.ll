; Reproduces NaN-sign-flip miscompile in performFMACombine FDOT2 fold
; (SIISelLowering.cpp:17729-17800). Distinct from m100 (which covers
; the denormal-mode issue at the same fold).
;
; The fold gates only on `contract` (and `dot10-insts`) -- no `nnan`
; check on either FMA's flags. The source IR lowers to two
; v_fma_mix_f32 ops; the folded form is one v_dot2c_f32_f16_e32.
; HW NaN-propagation differs:
;
;   v_fma_mix_f32:  propagates input NaN sign+payload per AMDGPU FMA rules.
;   v_dot2c_f32_f16: unconditionally SETS the sign bit of any NaN output,
;                    regardless of input sign or which operand is NaN.
;
; Sibling shape to m107/m120/m127/m110/m111 (SDAG NaN-sign-flip family)
; and m100 (same fold, denormal mode bug).
;
; Test value: a = <+qNaN_half, 1.0>, b = <1.0, 1.0>, z = 0.
;   Expected (-dot10-insts / two v_fma_mix_f32): +qNaN = 0x7FC00000
;   Observed (+dot10-insts / v_dot2c_f32_f16):    -qNaN = 0xFFC00000
;
; The bug also fires when NaN flows through `z` only (the accumulate
; step on v_dot2c_f32_f16 also forces sign=1 on any NaN output):
; a = <1, 1>, b = <1, 1>, z = +qNaN -> O2 stores -qNaN.
;
; Run with:
;   known-miscompiles/run_ll_reproducer.sh \
;       known-miscompiles/m128-performfmacombine-fdot2-flips-nan-sign/reduced.ll

source_filename = "m128-performfmacombine-fdot2-flips-nan-sign"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

declare float @llvm.fma.f32(float, float, float)

define amdgpu_kernel void @t(ptr addrspace(1) %out, <2 x half> %a, <2 x half> %b, float %z) {
  %ax = extractelement <2 x half> %a, i32 0
  %ay = extractelement <2 x half> %a, i32 1
  %bx = extractelement <2 x half> %b, i32 0
  %by = extractelement <2 x half> %b, i32 1
  %axf = fpext half %ax to float
  %ayf = fpext half %ay to float
  %bxf = fpext half %bx to float
  %byf = fpext half %by to float
  %inner = call contract float @llvm.fma.f32(float %ayf, float %byf, float %z)
  %outer = call contract float @llvm.fma.f32(float %axf, float %bxf, float %inner)
  store float %outer, ptr addrspace(1) %out
  ret void
}
