target triple = "x86_64-unknown-linux-gnu"
define i32 @test(i32 %x) {
entry:
  %s = shl i32 1, %x
  %f1 = freeze i32 %s
  %f2 = freeze i32 %s
  %d = sub i32 %f1, %f2
  ret i32 %d
}
