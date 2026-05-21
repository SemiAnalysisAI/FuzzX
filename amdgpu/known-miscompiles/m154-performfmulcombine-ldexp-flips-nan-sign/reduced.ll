; m154: performFMulCombine ldexp arm flips NaN sign via FNEG
; src-modifier on the LDEXP input.
;
; SIISelLowering.cpp:17684-17722 (performFMulCombine, ldexp arm):
;   fmul x, (select c, -A, -B)
;   ->
;   ldexp(fneg x, select c, log2|A|, log2|B|)
;
; Gate is only "TrueNode->isNegative() == FalseNode->isNegative()"
; and exact-log2 -- NO check on N->getFlags().hasNoNaNs().
;
; For x = NaN:
;   v_mul_f32(NaN, -K) -- output NaN sign == input NaN sign (m107).
;   v_ldexp_f32(-NaN, exp) -- src-modifier XORs sign; ldexp passes
;     NaN through unchanged.  Output NaN sign FLIPPED.
;
; Asm:
;   v_cndmask_b32_e64 v1, 3, 2, vcc
;   v_ldexp_f32 v0, -v0, v1     ; VOP3 NEG on src0
;
; Same family as m107 (FMUL arm), m110 (FMED3), m111 (VOP3P MadFmaMix),
; m120 (FMul fneg-LHS), m127 (FSub fadd folds), m128 (FDOT2),
; m139 (FMA arm), m140 (FADD arm).

source_filename = "m154-performfmulcombine-ldexp-flips-nan-sign"
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @t(ptr addrspace(1) %in, ptr addrspace(1) %out, i1 %c) {
  %xi = load i32, ptr addrspace(1) %in, align 4
  %x  = bitcast i32 %xi to float
  ; select between two negative pow-of-2 constants.  Combine fires:
  ; (fmul x, select c, -4.0, -8.0) -> ldexp(-x, select c, 2, 3).
  %s = select i1 %c, float -4.0, float -8.0
  %r = fmul float %x, %s
  store float %r, ptr addrspace(1) %out, align 4
  ret void
}
