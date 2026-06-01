target triple = "nvptx64-nvidia-cuda"
define ptx_kernel void @k(ptr addrspace(1) %in, ptr addrspace(1) %out) {
  %v = load atomic i32, ptr addrspace(1) %in monotonic, align 4
  store i32 %v, ptr addrspace(1) %out
  ret void
}
