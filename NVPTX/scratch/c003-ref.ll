; Reference: same math, but force the multiplier to be a wide positive constant.
define i32 @ref15(i16 %x) {
  %e = sext i16 %x to i32
  %m = shl i32 %e, 15
  ret i32 %m
}
define i64 @ref31(i32 %x) {
  %e = sext i32 %x to i64
  %m = shl i64 %e, 31
  ret i64 %m
}
