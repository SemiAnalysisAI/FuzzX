@g = external global i32

define void @test_then(i32 %x, i32 %y, i32 %z) {
entry.x:
  %cmp.x = icmp ne i32 %x, 0
  br i1 %cmp.x, label %if.then.x, label %entry.y, !prof !0

if.then.x:
  store i32 %z, ptr @g, align 4
  br label %entry.y

entry.y:
  %cmp.y = icmp ne i32 %y, 0
  br i1 %cmp.y, label %if.then.y, label %exit, !prof !1

if.then.y:
  store i32 %z, ptr @g, align 4
  br label %exit

exit:
  ret void
}

!0 = !{!"branch_weights", i32 1, i32 99}
!1 = !{!"branch_weights", i32 50, i32 50}
