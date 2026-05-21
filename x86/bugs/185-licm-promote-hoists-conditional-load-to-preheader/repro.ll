target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"
define i32 @test_cond_load(i32 %n, i1 %c, ptr %p) {
entry: br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %latch ]
  %sum = phi i32 [ 0, %entry ], [ %sum.next, %latch ]
  br i1 %c, label %then, label %skip
then:
  %v = load i32, ptr %p, align 4
  br label %skip
skip:
  %x = phi i32 [ %v, %then ], [ 0, %loop ]
  %sum.next = add i32 %sum, %x
  br label %latch
latch:
  store i32 %sum.next, ptr %p, align 4
  %i.next = add i32 %i, 1
  %cond = icmp slt i32 %i.next, %n
  br i1 %cond, label %loop, label %exit
exit: ret i32 %sum.next
}
