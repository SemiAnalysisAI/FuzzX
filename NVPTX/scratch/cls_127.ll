declare i127 @ext(i127 %x)
define i127 @call(i127 %x) {
  %r = call i127 @ext(i127 %x)
  ret i127 %r
}
