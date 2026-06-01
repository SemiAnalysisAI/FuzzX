target triple = "nvptx64-nvidia-cuda"

declare i32 @llvm.nvvm.f2tf32.rn.relu.satfinite(float)
declare i32 @llvm.nvvm.f2tf32.rz.relu.satfinite(float)

define i32 @t_rn_relu_satf(float %a) {
  %r = call i32 @llvm.nvvm.f2tf32.rn.relu.satfinite(float %a)
  ret i32 %r
}
define i32 @t_rz_relu_satf(float %a) {
  %r = call i32 @llvm.nvvm.f2tf32.rz.relu.satfinite(float %a)
  ret i32 %r
}
