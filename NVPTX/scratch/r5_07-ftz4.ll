target triple = "nvptx64-nvidia-cuda"
define float @fadd_ftz(float %a, float %b) #0 {
  %r = fadd float %a, %b
  ret float %r
}
attributes #0 = { "denormal-fp-math-f32"="preserve-sign" }
