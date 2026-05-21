target triple = "x86_64-unknown-linux-gnu"

define void @f(i32 %n) {
entry:
  br label %h

h:
  %i = phi i32 [0, %entry], [%i1, %be1], [%i2, %be2]
  %i1 = add i32 %i, 1
  %i2 = add i32 %i, 2
  %c1 = icmp ult i32 %i, %n
  br i1 %c1, label %be1, label %mid

mid:
  %c2 = icmp ult i32 %i, 100
  br i1 %c2, label %be2, label %exit

be1:
  br label %h, !llvm.loop !0

be2:
  br label %h, !llvm.loop !3

exit:
  ret void
}

!0 = distinct !{!0, !1, !2}
!1 = !{!"llvm.loop.unroll.count", i32 2}
!2 = !{!"llvm.loop.disable_nonforced"}
!3 = distinct !{!3, !4}
!4 = !{!"llvm.loop.mustprogress"}
