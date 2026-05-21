target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i16 @test(i32 %x) {
entry:
  %c1 = icmp uge i32 %x, 0
  %c2 = icmp ult i32 %x, 256
  %c  = and i1 %c1, %c2
  br i1 %c, label %then, label %end
then:
  %t = trunc i32 %x to i16
  ret i16 %t
end:
  ret i16 0
}
