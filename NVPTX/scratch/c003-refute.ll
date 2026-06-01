define i32 @sext_shl15(i16 %x) {
  %s = shl nsw i16 %x, 15
  %e = sext i16 %s to i32
  ret i32 %e
}

define i64 @sext_shl31(i32 %x) {
  %s = shl nsw i32 %x, 31
  %e = sext i32 %s to i64
  ret i64 %e
}
