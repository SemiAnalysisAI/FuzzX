define bfloat @i32_to_bf16(i32 %x) {
  %r = sitofp i32 %x to bfloat
  ret bfloat %r
}

define bfloat @i32_to_bf16_const() {
  %r = sitofp i32 33685505 to bfloat
  ret bfloat %r
}

define bfloat @i64_to_bf16(i64 %x) {
  %r = sitofp i64 %x to bfloat
  ret bfloat %r
}
