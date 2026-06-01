define i32 @sext_shl14(i16 %x) {
  %s = shl nsw i16 %x, 14
  %e = sext i16 %s to i32
  ret i32 %e
}
