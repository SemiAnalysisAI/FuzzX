target triple = "x86_64-unknown-linux-gnu"
%S = type { i32, i32 }
define void @f(ptr %p, %S %v) {
  store %S %v, ptr %p, align 4, !nontemporal !0
  ret void
}
!0 = !{i32 1}
