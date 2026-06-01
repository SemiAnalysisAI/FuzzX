target triple = "nvptx64-nvidia-cuda"

define void @add(ptr addrspace(7) %p, i32 %v) {
  %r = atomicrmw add ptr addrspace(7) %p, i32 %v monotonic
  ret void
}
define void @cas(ptr addrspace(7) %p, i32 %c, i32 %v) {
  %r = cmpxchg ptr addrspace(7) %p, i32 %c, i32 %v monotonic monotonic
  ret void
}
define void @exch(ptr addrspace(7) %p, i32 %v) {
  %r = atomicrmw xchg ptr addrspace(7) %p, i32 %v monotonic
  ret void
}
