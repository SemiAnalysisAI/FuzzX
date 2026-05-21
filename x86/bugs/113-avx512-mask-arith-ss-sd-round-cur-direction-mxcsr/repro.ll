target triple = "x86_64-unknown-unknown"
declare <4 x float> @llvm.x86.avx512.mask.add.ss.round(<4 x float>, <4 x float>, <4 x float>, i8, i32 immarg)
declare void @llvm.x86.sse.ldmxcsr(ptr)
define <4 x float> @t(<4 x float> %a, <4 x float> %b, ptr %p) {
  call void @llvm.x86.sse.ldmxcsr(ptr %p)
  %r = call <4 x float> @llvm.x86.avx512.mask.add.ss.round(<4 x float> %a, <4 x float> %b, <4 x float> zeroinitializer, i8 -1, i32 4)
  ret <4 x float> %r
}
