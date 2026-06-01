target triple = "nvptx64-nvidia-cuda"
define {i32, i1} @local_cas(ptr addrspace(5) %p, i32 %c, i32 %n) {
  %r = cmpxchg ptr addrspace(5) %p, i32 %c, i32 %n monotonic monotonic
  ret {i32, i1} %r
}
define i64 @cas64(ptr addrspace(5) %p, i64 %c, i64 %n) {
  %r = cmpxchg ptr addrspace(5) %p, i64 %c, i64 %n monotonic monotonic
  %v = extractvalue {i64,i1} %r, 0
  ret i64 %v
}
define i128 @cas128(ptr addrspace(5) %p, i128 %c, i128 %n) {
  %r = cmpxchg ptr addrspace(5) %p, i128 %c, i128 %n monotonic monotonic
  %v = extractvalue {i128,i1} %r, 0
  ret i128 %v
}
