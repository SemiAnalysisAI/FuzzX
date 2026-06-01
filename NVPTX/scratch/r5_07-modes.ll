target triple = "nvptx64-nvidia-cuda"

define float @fadd_ieee(ptr %p, float %v) #0 {
  %r = atomicrmw fadd ptr %p, float %v monotonic
  ret float %r
}

define float @fadd_ftz(ptr %p, float %v) #1 {
  %r = atomicrmw fadd ptr %p, float %v monotonic
  ret float %r
}

attributes #0 = { "denormal-fp-math"="ieee,ieee" }
attributes #1 = { "denormal-fp-math"="preserve-sign,preserve-sign" }
