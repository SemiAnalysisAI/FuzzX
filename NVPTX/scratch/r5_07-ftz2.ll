target triple = "nvptx64-nvidia-cuda"
define float @plain_fadd_ftz(float %a, float %b) #0 {
  %r = fadd float %a, %b
  ret float %r
}
attributes #0 = { "denormal-fp-math"="preserve-sign,preserve-sign" }
