target triple = "nvptx64-nvidia-cuda"

declare float @llvm.nvvm.add.rn.ftz.sat.f(float, float)
declare float @llvm.nvvm.add.rz.ftz.sat.f(float, float)

define float @add_rn_ftz_sat(float %a, float %b) {
  %r = call float @llvm.nvvm.add.rn.ftz.sat.f(float %a, float %b)
  ret float %r
}

define float @sub_rn_ftz_sat(float %a, float %b) {
  %nb = fneg float %b
  %r = call float @llvm.nvvm.add.rn.ftz.sat.f(float %a, float %nb)
  ret float %r
}
