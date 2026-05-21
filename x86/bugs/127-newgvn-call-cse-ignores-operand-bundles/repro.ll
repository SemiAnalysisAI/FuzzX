declare i32 @rd(i32) memory(read)
define i32 @test(i32 %x) {
  %a = call i32 @rd(i32 %x)
  %b = call i32 @rd(i32 %x) [ "deopt"() ]
  %r = add i32 %a, %b
  ret i32 %r
}
