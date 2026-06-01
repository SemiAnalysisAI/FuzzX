define ptx_kernel void @kern(i65 %x, ptr %out) {
  %t = trunc i65 %x to i64
  store i64 %t, ptr %out
  ret void
}
