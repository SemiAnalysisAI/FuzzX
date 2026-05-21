target triple = "x86_64-unknown-linux-gnu"
%S = type { i32, i32 }
define %S @f(ptr %p) {
  %v = load %S, ptr %p, align 4, !nontemporal !0
  ret %S %v
}
!0 = !{i32 1}
