target triple = "nvptx64-nvidia-cuda"
define float @plain_fadd(float %a, float %b) {
  %r = fadd float %a, %b
  ret float %r
}
define float @atomic_fadd(ptr %p, float %v) {
  %r = atomicrmw fadd ptr %p, float %v monotonic
  ret float %r
}
