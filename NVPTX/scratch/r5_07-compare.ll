target triple = "nvptx64-nvidia-cuda"
define float @plain_fadd(float %a, float %b) {
  %r = fadd float %a, %b
  ret float %r
}
define float @plain_fadd_ftz(float %a, float %b) #0 {
  %r = fadd float %a, %b
  ret float %r
}
define float @fadd_f32_ftz(ptr %p, float %v) #0 {
  %r = atomicrmw fadd ptr %p, float %v monotonic
  ret float %r
}
attributes #0 = { "denormal-fp-math-f32"="preserve-sign,preserve-sign" }
