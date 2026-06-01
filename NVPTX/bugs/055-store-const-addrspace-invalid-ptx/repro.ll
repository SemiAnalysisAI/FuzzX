target triple = "nvptx64-nvidia-cuda"
define void @store_const(ptr addrspace(4) %p, i32 %v) {
  store i32 %v, ptr addrspace(4) %p
  ret void
}
