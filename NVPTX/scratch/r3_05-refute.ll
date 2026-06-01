define i32 @guarded_shift_wide(i32 %x, i64 %s) {
  %st = trunc i64 %s to i32
  %c = icmp ugt i64 %s, 31
  %sh = lshr i32 %x, %st
  %r = select i1 %c, i32 0, i32 %sh
  ret i32 %r
}

define i32 @guarded_shl_wide_ult(i32 %x, i64 %s) {
  %st = trunc i64 %s to i32
  %c = icmp ult i64 %s, 32
  %sh = shl i32 %x, %st
  %r = select i1 %c, i32 %sh, i32 0
  ret i32 %r
}
