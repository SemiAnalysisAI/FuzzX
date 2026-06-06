define i1 @icmp_or_xor_oneuse_fixpoint(i64 %x1, i64 %y1, i64 %x2, i64 %y2) {
  %xor = xor i64 %x1, %y1
  %xor1 = xor i64 %x2, %y2
  %or = or i64 %xor, %xor1
  %cmp = icmp eq i64 %or, 0
  %cmp_1 = icmp eq i64 %xor, 0
  %or1 = or i1 %cmp, %cmp_1
  ret i1 %or1
}
