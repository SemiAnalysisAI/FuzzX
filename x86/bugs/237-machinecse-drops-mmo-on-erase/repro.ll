target triple = "x86_64-unknown-linux-gnu"
define i32 @f(ptr %p, i1 %c) {
entry:
  br i1 %c, label %t, label %f
t:
  %a = load i32, ptr %p, align 4, !range !0
  br label %end
f:
  %b = load i32, ptr %p, align 4, !range !1
  br label %end
end:
  %v = phi i32 [ %a, %t ], [ %b, %f ]
  ret i32 %v
}
!0 = !{i32 0, i32 10}
!1 = !{i32 20, i32 30}
