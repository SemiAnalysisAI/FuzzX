target triple = "x86_64-unknown-linux-gnu"
define i32 @f(ptr %p) {
  %a = load i32, ptr %p, align 4, !nontemporal !0
  %b = load i32, ptr %p, align 4
  %c = add i32 %a, %b
  ret i32 %c
}
!0 = !{i32 1}
