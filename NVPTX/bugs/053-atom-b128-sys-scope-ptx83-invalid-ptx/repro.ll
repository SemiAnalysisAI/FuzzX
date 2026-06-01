target triple = "nvptx64-nvidia-cuda"

define i128 @xchg128(ptr %p, i128 %v) {
  %r = atomicrmw xchg ptr %p, i128 %v monotonic
  ret i128 %r
}

define i128 @cas128(ptr %p, i128 %c, i128 %v) {
  %r = cmpxchg ptr %p, i128 %c, i128 %v monotonic monotonic
  %x = extractvalue { i128, i1 } %r, 0
  ret i128 %x
}
