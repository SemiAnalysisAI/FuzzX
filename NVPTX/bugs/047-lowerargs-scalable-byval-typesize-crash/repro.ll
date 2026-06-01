target triple = "nvptx64-nvidia-cuda"
define ptx_kernel void @kern(ptr byval(<vscale x 4 x i32>) align 16 %p, ptr addrspace(1) %out) {
entry:
  store i32 1, ptr %p, align 4
  ret void
}
