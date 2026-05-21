target triple = "x86_64-unknown-linux-gnu"
define i128 @f(ptr %p) {
  %l = load i128, ptr %p, align 16, !range !0
  ret i128 %l
}
!0 = !{i128 0, i128 100}
