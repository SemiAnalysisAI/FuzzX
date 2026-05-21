target triple = "x86_64-unknown-linux-gnu"
define void @f(ptr %p) {
  store i32 0, ptr %p, align 4, !nontemporal !1   ; nontemporal store of 0
  store i32 0, ptr %p, align 4                    ; redundant store of same value 0, no nontemporal
  ret void
}
!1 = !{i32 1}
