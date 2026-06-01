declare i72 @ext(i72 %x)
define i72 @call(i72 %x) {
  %r = call i72 @ext(i72 %x)
  ret i72 %r
}
