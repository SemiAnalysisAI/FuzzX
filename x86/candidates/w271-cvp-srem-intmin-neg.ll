target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @test(i32 %x, i32 %y) {
entry:
  %cx = icmp sle i32 %x, 0
  %cy = icmp sgt i32 %y, 0
  %c  = and i1 %cx, %cy
  br i1 %c, label %then, label %end
then:
  %r = srem i32 %x, %y
  ret i32 %r
end:
  ret i32 0
}
