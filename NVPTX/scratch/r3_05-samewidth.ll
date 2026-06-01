; same-width i32 guard + i32 amount (verifier says this is correct)
define i32 @samewidth(i32 %x, i32 %s) {
  %c = icmp ugt i32 %s, 31
  %sh = lshr i32 %x, %s
  %r = select i1 %c, i32 0, i32 %sh
  ret i32 %r
}
