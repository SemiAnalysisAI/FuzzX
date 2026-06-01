define ptx_kernel void @k_i3(i3 %x, ptr %o) {
  store i3 %x, ptr %o
  ret void
}
