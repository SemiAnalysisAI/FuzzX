target triple = "x86_64-unknown-linux-gnu"
define void @hoist_fence_acquire(i32 %n) {
entry: br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  fence acquire
  %i.next = add i32 %i, 1
  %cond = icmp slt i32 %i.next, %n
  br i1 %cond, label %loop, label %exit
exit: ret void
}
define void @hoist_fence_seqcst(i32 %n) {
entry: br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  fence seq_cst
  %i.next = add i32 %i, 1
  %cond = icmp slt i32 %i.next, %n
  br i1 %cond, label %loop, label %exit
exit: ret void
}
