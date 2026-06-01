target triple = "nvptx64-nvidia-cuda"
define float @fadd_ftz(float %a, float %b) #0 {
  %r = fadd float %a, %b
  ret float %r
}
define float @atomic_ftz(ptr %p, float %v) #0 {
  %r = atomicrmw fadd ptr %p, float %v monotonic
  ret float %r
}
attributes #0 = { "denormal-fp-math"="preserve-sign,preserve-sign" }
