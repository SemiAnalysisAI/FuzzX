define i32 @test(i32 %x) {
  %s = shl i32 1, %x
  %f1 = freeze i32 %s
  %f2 = freeze i32 %s
  %d = sub i32 %f1, %f2
  ret i32 %d
}
