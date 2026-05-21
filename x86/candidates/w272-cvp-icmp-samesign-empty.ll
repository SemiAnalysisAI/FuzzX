target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i1 @test(i32 %y) {
entry:
  %u = freeze i32 undef
  %and_u = and i32 %u, 127
  %and_y = and i32 %y, 127
  %r = icmp slt i32 %and_u, %and_y
  ret i1 %r
}
