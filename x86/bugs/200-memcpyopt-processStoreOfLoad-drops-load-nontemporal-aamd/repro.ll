target triple = "x86_64-unknown-linux-gnu"
%S = type { i64, i64, i64, i64 }
declare void @use(ptr)
define void @test(ptr noalias %dst, ptr noalias %src) {
  %x = load %S, ptr %src, align 8, !nontemporal !0
  store %S %x, ptr %dst, align 8
  call void @use(ptr %dst)
  ret void
}
!0 = !{i32 1}
