target triple = "nvptx64-nvidia-cuda"

declare i16 @llvm.nvvm.atomic.cas.gen.i.cta.i16.p0(ptr, i16, i16)

define i16 @cas16(ptr %p, i16 %cmp, i16 %new) {
  %r = call i16 @llvm.nvvm.atomic.cas.gen.i.cta.i16.p0(ptr %p, i16 %cmp, i16 %new)
  ret i16 %r
}
