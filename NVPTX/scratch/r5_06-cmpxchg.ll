define i32 @cx(ptr %p, i32 %c, i32 %s) {
  %r = cmpxchg ptr %p, i32 %c, i32 %s syncscope("block") monotonic monotonic
  %v = extractvalue {i32, i1} %r, 0
  ret i32 %v
}
