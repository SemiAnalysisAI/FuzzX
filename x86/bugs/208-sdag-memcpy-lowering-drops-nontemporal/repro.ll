target triple = "x86_64-unknown-linux-gnu"
declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)
define void @f(ptr %d, ptr %s) {
  call void @llvm.memcpy.p0.p0.i64(ptr align 16 %d, ptr align 16 %s, i64 64, i1 false), !nontemporal !0
  ret void
}
!0 = !{i32 1}
