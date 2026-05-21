target triple = "x86_64-unknown-linux-gnu"
declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)
declare void @use(ptr)
define void @test(ptr %dst, ptr %mid, ptr %src) {
  call void @llvm.memcpy.p0.p0.i64(ptr %mid, ptr %src, i64 32, i1 false), !nontemporal !0
  call void @llvm.memcpy.p0.p0.i64(ptr %dst, ptr %mid, i64 32, i1 false)
  call void @use(ptr %dst)
  ret void
}
!0 = !{i32 1}
