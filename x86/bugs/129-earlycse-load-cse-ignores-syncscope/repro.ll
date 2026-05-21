target triple = "x86_64-unknown-linux-gnu"
define i32 @f(ptr %p) {
  %a = load atomic i32, ptr %p syncscope("singlethread") unordered, align 4
  %b = load atomic i32, ptr %p syncscope("system") unordered, align 4
  %r = add i32 %a, %b
  ret i32 %r
}
