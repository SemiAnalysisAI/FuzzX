define ptx_kernel void @k_i24(i24 %x, ptr %o) {
  store i24 %x, ptr %o
  ret void
}
