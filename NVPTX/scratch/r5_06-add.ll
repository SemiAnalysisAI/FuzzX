declare i32 @llvm.nvvm.atomic.add.gen.i.cta.i32.p0(ptr, i32)
define i32 @scoped_add_i32_cta(ptr %p, i32 %v) {
  %r = call i32 @llvm.nvvm.atomic.add.gen.i.cta.i32.p0(ptr %p, i32 %v)
  ret i32 %r
}
