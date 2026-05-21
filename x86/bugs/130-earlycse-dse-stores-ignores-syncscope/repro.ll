target triple = "x86_64-unknown-linux-gnu"
define void @g(ptr %p, i32 %v) {
  store atomic i32 %v, ptr %p syncscope("singlethread") unordered, align 4
  store atomic i32 %v, ptr %p syncscope("system") unordered, align 4
  ret void
}
