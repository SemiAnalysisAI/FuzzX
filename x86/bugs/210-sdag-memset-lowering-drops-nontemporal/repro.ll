target triple = "x86_64-unknown-linux-gnu"
declare void @llvm.memset.p0.i64(ptr, i8, i64, i1)
define void @f(ptr %d) {
  call void @llvm.memset.p0.i64(ptr align 16 %d, i8 0, i64 64, i1 false), !nontemporal !0
  ret void
}
!0 = !{i32 1}
