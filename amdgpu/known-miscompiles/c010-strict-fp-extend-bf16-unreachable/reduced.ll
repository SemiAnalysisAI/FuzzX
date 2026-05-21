; c010: STRICT_FP_EXTEND bf16 -> f32/f64 hits llvm_unreachable in
; SIISelLowering::lowerFP_EXTEND on gfx950.
;
; STRICT_FP_EXTEND is set Custom for f32/f64 dst (SIISelLowering.cpp:580-581).
; lowerFP_EXTEND detects bf16 source then llvm_unreachable
; ("Need STRICT_BF16_TO_FP") at lines 4914-4915 for the strict case.
;
; Crashes a release-asserts compiler on any
; llvm.experimental.constrained.fpext.f32.bf16 (or .f64.bf16).
;
; Sibling to c001/c003/c006/c008 (intrinsic without selector for
; relevant target generation) and m143 (STRICT_FP_ROUND f64->bf16
; drops chain).

source_filename = "c010-strict-fp-extend-bf16-unreachable"
target triple = "amdgcn-amd-amdhsa"

declare float @llvm.experimental.constrained.fpext.f32.bf16(bfloat, metadata)

define amdgpu_kernel void @t(ptr addrspace(1) %p, bfloat %x) #0 {
  %r = call float @llvm.experimental.constrained.fpext.f32.bf16(
         bfloat %x,
         metadata !"fpexcept.strict") #0
  store float %r, ptr addrspace(1) %p, align 4
  ret void
}

attributes #0 = { strictfp }
