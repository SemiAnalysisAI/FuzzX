define i1 @f() {
  %r = fcmp nnan ord double 0x7FF8000000000000, 0.0
  ret i1 %r
}
