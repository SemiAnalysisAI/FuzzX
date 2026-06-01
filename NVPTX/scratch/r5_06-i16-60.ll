declare i16 @llvm.nvvm.atomic.cas.gen.i.cta.i16.p0(ptr, i16, i16)
define i16 @f(ptr %p, i16 %c, i16 %s) {
  %r = call i16 @llvm.nvvm.atomic.cas.gen.i.cta.i16.p0(ptr %p, i16 %c, i16 %s)
  ret i16 %r
}
