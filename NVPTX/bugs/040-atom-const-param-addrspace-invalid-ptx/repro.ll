target triple = "nvptx64-nvidia-cuda"
define i32 @c_add(ptr addrspace(4) %p, i32 %v) {
  %r = atomicrmw add ptr addrspace(4) %p, i32 %v monotonic
  ret i32 %r
}
define i32 @p_add(ptr addrspace(101) %p, i32 %v) {
  %r = atomicrmw add ptr addrspace(101) %p, i32 %v monotonic
  ret i32 %r
}
define i32 @c_xchg(ptr addrspace(4) %p, i32 %v) {
  %r = atomicrmw xchg ptr addrspace(4) %p, i32 %v monotonic
  ret i32 %r
}
define {i32,i1} @c_cas(ptr addrspace(4) %p, i32 %c, i32 %n) {
  %r = cmpxchg ptr addrspace(4) %p, i32 %c, i32 %n monotonic monotonic
  ret {i32,i1} %r
}
