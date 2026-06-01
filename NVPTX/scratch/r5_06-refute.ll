declare i32 @llvm.nvvm.atomic.cas.gen.i.cta.i32.p0(ptr, i32, i32)
declare i32 @llvm.nvvm.atomic.cas.gen.i.sys.i32.p0(ptr, i32, i32)
declare i16 @llvm.nvvm.atomic.cas.gen.i.cta.i16.p0(ptr, i16, i16)

define i32 @scoped_cas_i32_cta(ptr %p, i32 %c, i32 %s) {
  %r = call i32 @llvm.nvvm.atomic.cas.gen.i.cta.i32.p0(ptr %p, i32 %c, i32 %s)
  ret i32 %r
}

define i32 @scoped_cas_i32_sys(ptr %p, i32 %c, i32 %s) {
  %r = call i32 @llvm.nvvm.atomic.cas.gen.i.sys.i32.p0(ptr %p, i32 %c, i32 %s)
  ret i32 %r
}

define i16 @scoped_cas_i16_cta(ptr %p, i16 %c, i16 %s) {
  %r = call i16 @llvm.nvvm.atomic.cas.gen.i.cta.i16.p0(ptr %p, i16 %c, i16 %s)
  ret i16 %r
}
