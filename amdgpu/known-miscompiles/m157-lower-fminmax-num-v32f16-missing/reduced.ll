; m157: lowerFMINIMUMNUM_FMAXIMUMNUM v32f16 missing handler case.
;
; SIISelLowering.cpp:826-829 sets ISD::FMINIMUMNUM and ISD::FMAXIMUMNUM
; to Custom for {v4f16, v8f16, v16f16, v32f16}.
;
; lowerFMINIMUMNUM_FMAXIMUMNUM (SIISelLowering.cpp:8637-8651) only
; calls splitBinaryVectorOp for {v4f16, v8f16, v16f16, v16bf16}.
; v32f16 is missing.
;
; In non-IEEE mode, a v32f16 op falls through to `return Op;` at line
; 8650, returning the original Custom-marked node unchanged.  The
; legalizer treats this as "no change" and either loops or asserts.
;
; Bonus oddity: the handler lists v16bf16, but no setOperationAction
; in 200-1000 marks FMINIMUMNUM/FMAXIMUMNUM Custom for any bf16
; vector -- that branch is dead.
;
; Reachability: triggerable in SDAG, gfx950, via
; llvm.minimumnum.v32f16 / llvm.maximumnum.v32f16 intrinsic.
;
; Note: the analogous lowerFMINNUM_FMAXNUM (line 8630) has the same
; v32f16 omission, but FMINNUM/FMAXNUM for v32f16 is set Expand
; (line 831), so it never reaches the Custom handler -- not a bug for
; FMINNUM/FMAXNUM, only for FMINIMUMNUM/FMAXIMUMNUM.

source_filename = "m157-lower-fminmax-num-v32f16-missing"
target triple = "amdgcn-amd-amdhsa"

declare <32 x half> @llvm.minimumnum.v32f16(<32 x half>, <32 x half>)
declare <32 x half> @llvm.maximumnum.v32f16(<32 x half>, <32 x half>)

define amdgpu_kernel void @t_min(ptr addrspace(1) %p, <32 x half> %a, <32 x half> %b) {
  %r = call <32 x half> @llvm.minimumnum.v32f16(<32 x half> %a, <32 x half> %b)
  store <32 x half> %r, ptr addrspace(1) %p, align 64
  ret void
}

define amdgpu_kernel void @t_max(ptr addrspace(1) %p, <32 x half> %a, <32 x half> %b) {
  %r = call <32 x half> @llvm.maximumnum.v32f16(<32 x half> %a, <32 x half> %b)
  store <32 x half> %r, ptr addrspace(1) %p, align 64
  ret void
}
