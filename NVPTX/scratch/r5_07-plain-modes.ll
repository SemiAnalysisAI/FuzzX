target triple = "nvptx64-nvidia-cuda"

define float @plain_ieee(float %a, float %b) #0 {
  %r = fadd float %a, %b
  ret float %r
}
define float @plain_ftz(float %a, float %b) #1 {
  %r = fadd float %a, %b
  ret float %r
}
attributes #0 = { "denormal-fp-math"="ieee,ieee" }
attributes #1 = { "denormal-fp-math"="preserve-sign,preserve-sign" }
