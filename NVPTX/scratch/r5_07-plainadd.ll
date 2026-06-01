target triple = "nvptx64-nvidia-cuda"
define float @plain_fadd_f32(float %a, float %b) {
  %r = fadd float %a, %b
  ret float %r
}
