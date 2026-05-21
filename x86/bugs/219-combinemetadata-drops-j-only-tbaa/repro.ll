target triple = "x86_64-unknown-linux-gnu"
define i32 @cse(ptr %p) {
  %a = load i32, ptr %p, align 4, !tbaa !0
  %b = load i32, ptr %p, align 4
  %s = add i32 %a, %b
  ret i32 %s
}
!0 = !{!1, !1, i64 0}
!1 = !{!"int", !2}
!2 = !{!"root"}
