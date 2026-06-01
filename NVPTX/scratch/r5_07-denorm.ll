target triple = "nvptx64-nvidia-cuda"
; No denormal-fp-math attribute => default mode = ieee (preserve subnormals)
define float @t(ptr %p, float %v) {
  %r = atomicrmw fadd ptr %p, float %v monotonic
  ret float %r
}
