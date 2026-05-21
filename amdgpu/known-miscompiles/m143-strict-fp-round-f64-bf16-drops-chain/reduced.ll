; m143: STRICT_FP_ROUND f64 -> bf16 silently drops strict chain and
; FP exception semantics on gfx950.
;
; SIISelLowering::lowerFP_ROUND (SIISelLowering.cpp:8604-8613) only
; short-circuits strict ops on the f64 -> f16 path (lines 8585-8587,
; "TODO: Handle strictfp").  The f64 -> bf16 path falls through the
; `assert(DstVT.getScalarType() == MVT::bf16)` without any strict
; guard:
;
;   1. expandRoundInexactToOdd (TargetLowering.cpp:12840) builds a
;      non-strict graph (FP_EXTEND/FABS/setcc/arithmetic) -- none of
;      which carry the strict chain or raise FP exceptions.
;   2. Emits a non-strict ISD::FP_ROUND at line 8612.
;
; Result: the strict node's chain (operand 0) is never threaded
; through, the second-result chain is silently lost, and exception
; semantics from the double-rounding-to-odd dance + final round are
; dropped.  Downstream nodes that should be ordered after the strict
; round can move arbitrarily.
;
; This reproducer uses llvm.experimental.constrained.fptrunc to take
; f64 -> bf16 with default rounding + "fpexcept.strict" semantics.
; On gfx950 the STRICT_FP_ROUND node is custom-lowered via the buggy
; path; the resulting DAG loses the strict chain.

source_filename = "m143-strict-fp-round-f64-bf16-drops-chain"
target triple = "amdgcn-amd-amdhsa"

declare bfloat @llvm.experimental.constrained.fptrunc.bf16.f64(double, metadata, metadata)

define amdgpu_kernel void @t(ptr addrspace(1) %p, double %x) #0 {
  %r = call bfloat @llvm.experimental.constrained.fptrunc.bf16.f64(
         double %x,
         metadata !"round.tonearest",
         metadata !"fpexcept.strict") #0
  store bfloat %r, ptr addrspace(1) %p, align 2
  ret void
}

attributes #0 = { strictfp }
