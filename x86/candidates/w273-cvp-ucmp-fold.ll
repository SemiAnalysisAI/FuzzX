target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare i32 @llvm.ucmp.i32.i32(i32, i32)

define i32 @test(i32 %x, i32 %y) {
entry:
  ; x in [10, 20), y in [50, 100)
  %cx1 = icmp uge i32 %x, 10
  %cx2 = icmp ult i32 %x, 20
  %cy1 = icmp uge i32 %y, 50
  %cy2 = icmp ult i32 %y, 100
  %c1 = and i1 %cx1, %cx2
  %c2 = and i1 %cy1, %cy2
  %cc = and i1 %c1, %c2
  br i1 %cc, label %then, label %end
then:
  %r = call i32 @llvm.ucmp.i32.i32(i32 %x, i32 %y)
  ret i32 %r
end:
  ret i32 0
}
