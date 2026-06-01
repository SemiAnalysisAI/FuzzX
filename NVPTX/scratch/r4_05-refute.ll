declare i72 @ext_i72(i72 %x)

define i72 @call_i72(i72 %x) {
  %r = call i72 @ext_i72(i72 %x)
  ret i72 %r
}
