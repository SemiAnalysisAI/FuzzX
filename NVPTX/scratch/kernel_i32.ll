define ptx_kernel void @k_i32(i32 %x, ptr %o) {
  store i32 %x, ptr %o
  ret void
}
