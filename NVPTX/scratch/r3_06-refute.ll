define i64 @passthru(i64 %x) {
  %v = call i64 asm "mov.u64 $0, $1;", "=r,r"(i64 %x)
  ret i64 %v
}
