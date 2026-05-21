target triple = "x86_64-unknown-linux-gnu"
@G = global i32 0, align 4
define i32 @ss_mismatch(i32 %n) {
entry: br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %sum = phi i32 [ 0, %entry ], [ %sum.next, %loop ]
  %v = load atomic i32, ptr @G syncscope("singlethread") unordered, align 4
  %sum.next = add i32 %sum, %v
  store atomic i32 %sum.next, ptr @G unordered, align 4
  %i.next = add i32 %i, 1
  %cond = icmp slt i32 %i.next, %n
  br i1 %cond, label %loop, label %exit
exit: ret i32 %sum.next
}
