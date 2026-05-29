define i32 @f(i32 %n, ptr %p) {
entry:
  br label %loop
loop:
  %iv = phi i32 [ 0, %entry ], [ %iv.next, %loop ]
  %sum = add nsw i32 %iv, 5
  %cmp = icmp samesign slt i32 %sum, 100
  %sel = select i1 %cmp, i32 1, i32 0
  store volatile i32 %sel, ptr %p
  %iv.next = add nsw i32 %iv, 1
  %done = icmp eq i32 %iv.next, %n
  br i1 %done, label %exit, label %loop
exit:
  ret i32 0
}
