target triple = "x86_64-unknown-linux-gnu"
define void @f(i1 %c, ptr %p, i32 %x) {
entry:
  br i1 %c, label %a, label %b
a:
  store atomic i32 %x, ptr %p monotonic, align 4
  br label %end
b:
  store atomic i32 %x, ptr %p syncscope("singlethread") monotonic, align 4
  br label %end
end:
  ret void
}
