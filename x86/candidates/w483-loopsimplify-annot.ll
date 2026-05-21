target triple = "x86_64-unknown-linux-gnu"

define i32 @f(i32 %n, i1 %p, i32 %m) {
entry:
  br i1 %p, label %h, label %exit, !annotation !2

h:
  %i = phi i32 [0, %entry], [%inc, %lat]
  %c = icmp slt i32 %i, %n
  br i1 %c, label %lat, label %exit, !annotation !3

lat:
  %inc = add i32 %i, 1
  br label %h

exit:
  %r = phi i32 [%m, %entry], [%i, %h]
  ret i32 %r
}

!2 = !{!"entry_annot"}
!3 = !{!"loop_exit_annot"}
