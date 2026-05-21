declare void @llvm.memset.p0.i64(ptr, i8, i64, i1)
declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)
declare void @use(ptr)
define void @test1(ptr %src) {
  %dst = alloca [32 x i8], align 8
  call void @llvm.memset.p0.i64(ptr align 8 %dst, i8 0, i64 32, i1 true)
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %dst, ptr align 8 %src, i64 16, i1 false)
  call void @use(ptr %dst)
  ret void
}
