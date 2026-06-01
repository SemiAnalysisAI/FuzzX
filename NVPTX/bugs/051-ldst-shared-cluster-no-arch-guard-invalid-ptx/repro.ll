target triple = "nvptx64-nvidia-cuda"
define void @st_cluster(ptr addrspace(7) %p, i32 %v) {
  store i32 %v, ptr addrspace(7) %p
  ret void
}
define i32 @ld_cluster(ptr addrspace(7) %p) {
  %v = load i32, ptr addrspace(7) %p
  ret i32 %v
}
