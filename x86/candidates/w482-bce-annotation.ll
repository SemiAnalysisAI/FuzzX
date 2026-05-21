target triple = "x86_64-unknown-linux-gnu"

define i32 @f(i1 %c, i32 %x, i32 %y) {
entry:
  br i1 %c, label %t, label %m, !annotation !0, !prof !1

t:
  %a = add i32 %x, 1
  br label %m

m:
  %p = phi i32 [ %a, %t ], [ %y, %entry ]
  ret i32 %p
}

!0 = !{!"my_annotation"}
!1 = !{!"branch_weights", i32 1, i32 100}
