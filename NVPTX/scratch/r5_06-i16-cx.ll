define i16 @g(ptr %p, i16 %c, i16 %s) {
  %r = cmpxchg ptr %p, i16 %c, i16 %s monotonic monotonic
  %v = extractvalue {i16, i1} %r, 0
  ret i16 %v
}
