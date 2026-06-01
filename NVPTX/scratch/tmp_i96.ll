declare i96 @ext(i96 %x)
define i96 @call_i96(i96 %x) {
  %r = call i96 @ext(i96 %x)
  ret i96 %r
}
