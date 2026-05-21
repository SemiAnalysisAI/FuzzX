target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @test(i32 %x) {
entry:
  %cx = icmp sle i32 %x, 0
  br i1 %cx, label %then, label %end
then:
  %d = sdiv exact i32 %x, -1
  ret i32 %d
end:
  ret i32 0
}
