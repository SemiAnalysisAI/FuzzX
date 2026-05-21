target triple = "x86_64-unknown-linux-gnu"
define i32 @f(ptr %p) {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %inext, %loop ]
  %s = phi i32 [ 0, %entry ], [ %snext, %loop ]
  %l1 = load i32, ptr %p, align 4
  %l2 = load i32, ptr %p, align 4, !nontemporal !0
  %v = add i32 %l1, %l2
  %snext = add i32 %s, %v
  %inext = add i32 %i, 1
  %c = icmp slt i32 %inext, 8
  br i1 %c, label %loop, label %exit
exit:
  ret i32 %snext
}
!0 = !{i32 1}
