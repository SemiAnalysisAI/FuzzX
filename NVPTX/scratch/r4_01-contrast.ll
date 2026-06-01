define ptx_kernel void @k_i32(i32 %x, ptr %o) {
  store i32 %x, ptr %o
  ret void
}
define ptx_kernel void @k_i24(i24 %x, ptr %o) {
  store i24 %x, ptr %o
  ret void
}
define ptx_kernel void @k_i3(i3 %x, ptr %o) {
  store i3 %x, ptr %o
  ret void
}
define void @nonkernel_i48(i48 %x, ptr %o) {
  store i48 %x, ptr %o
  ret void
}
