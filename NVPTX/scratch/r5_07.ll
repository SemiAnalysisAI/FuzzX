target triple = "nvptx64-nvidia-cuda"
define float @fadd_f32_ieee(ptr %p, float %v) {
  %r = atomicrmw fadd ptr %p, float %v monotonic
  ret float %r
}
