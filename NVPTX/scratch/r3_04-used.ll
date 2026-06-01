target triple = "nvptx64-nvidia-cuda"

define internal ptx_kernel void @ik(ptr byval(i32) %p) {
  ret void
}

define ptx_kernel void @caller(ptr byval(i32) %q) {
  call void @ik(ptr byval(i32) %q)
  ret void
}
