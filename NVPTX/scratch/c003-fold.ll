define i32 @sext_shl15_m1() {
  %s = shl nsw i16 -1, 15
  %e = sext i16 %s to i32
  ret i32 %e
}

define i64 @sext_shl31_m1() {
  %s = shl nsw i32 -1, 31
  %e = sext i32 %s to i64
  ret i64 %e
}
