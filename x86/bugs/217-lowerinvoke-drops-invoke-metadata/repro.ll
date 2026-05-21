target triple = "x86_64-unknown-linux-gnu"
declare i32 @callee()
define i32 @f() personality ptr null {
  %r = invoke i32 @callee() to label %cont unwind label %ueh, !prof !0, !annotation !1, !range !2
cont:
  ret i32 %r
ueh:
  %eh = landingpad { ptr, i32 } cleanup
  ret i32 0
}
!0 = !{!"branch_weights", i32 100, i32 1}
!1 = !{!"annot"}
!2 = !{i32 0, i32 100}
