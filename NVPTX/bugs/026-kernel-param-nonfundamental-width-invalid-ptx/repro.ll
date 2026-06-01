define ptx_kernel void @k_i48(i48 %x, ptr %o) {
  store i48 %x, ptr %o
  ret void
}
