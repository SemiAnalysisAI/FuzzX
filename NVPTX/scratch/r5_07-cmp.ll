target triple = "nvptx64-nvidia-cuda"

; plain fadd, default (ieee) denormal mode
define float @plain_fadd(float %a, float %b) {
  %r = fadd float %a, %b
  ret float %r
}

; atomicrmw fadd with explicit ieee denormal mode
define float @fadd_f32_explicit_ieee(ptr %p, float %v) "denormal-fp-math-f32"="ieee,ieee" {
  %r = atomicrmw fadd ptr %p, float %v monotonic
  ret float %r
}

; atomicrmw fadd with preserve-sign (FTZ) denormal mode
define float @fadd_f32_ftz(ptr %p, float %v) "denormal-fp-math-f32"="preserve-sign,preserve-sign" {
  %r = atomicrmw fadd ptr %p, float %v monotonic
  ret float %r
}

; double for comparison
define double @fadd_f64(ptr %p, double %v) {
  %r = atomicrmw fadd ptr %p, double %v monotonic
  ret double %r
}
