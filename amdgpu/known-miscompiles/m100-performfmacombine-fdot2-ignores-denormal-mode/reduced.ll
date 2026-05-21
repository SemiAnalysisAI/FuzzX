; gfx950: performFMACombine permits FDOT2 fold only on AllowContract.
; But fdot2_f32_f16 always FTZs the f32 operand and the output, regardless
; of denormal-fp-math-f32. The pre-fold FMA chain (v_fma_mix_f32) honors
; the mode. Compiling with denormal preserving and contract should keep
; the denormal output. The combine drops it on the floor.

target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @t(ptr addrspace(1) %out, <2 x half> %a, <2 x half> %b, float %z) #0 {
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
declare float @llvm.fma.f32(float, float, float)
attributes #0 = { "denormal-fp-math-f32"="ieee,ieee" }
