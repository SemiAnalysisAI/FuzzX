define i32 @f(i32 %x) {
  %s = shl i32 1, %x  ; poison when x >= 32
  %f1 = freeze i32 %s
  %f2 = freeze i32 %s
  %d = sub i32 %f1, %f2
  ret i32 %d
}
