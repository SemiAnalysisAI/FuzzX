declare i65 @ext(i65 %x)
define i65 @call(i65 %x) {
  %r = call i65 @ext(i65 %x)
  ret i65 %r
}
