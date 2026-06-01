target triple = "nvptx64-nvidia-cuda"
declare <2 x bfloat> @llvm.nvvm.ff2bf16x2.rn(float, float)
define <2 x bfloat> @f(float %a, float %b) {
  %r = call <2 x bfloat> @llvm.nvvm.ff2bf16x2.rn(float %a, float %b)
  ret <2 x bfloat> %r
}
