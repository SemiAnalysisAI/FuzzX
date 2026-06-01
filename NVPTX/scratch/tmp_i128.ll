declare i128 @ext(i128 %x)
define i128 @call_i128(i128 %x) {
  %r = call i128 @ext(i128 %x)
  ret i128 %r
}
