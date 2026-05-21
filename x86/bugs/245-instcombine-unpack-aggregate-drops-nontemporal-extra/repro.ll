target triple = "x86_64-unknown-linux-gnu"
%S = type { i64, i64 }
define void @f(ptr %p, %S %v) {
  store %S %v, ptr %p, align 8, !nontemporal !0, !invariant.group !1
  ret void
}
!0 = !{i32 1}
!1 = !{}
