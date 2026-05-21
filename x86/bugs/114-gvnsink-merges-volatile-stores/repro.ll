target triple = "x86_64-unknown-linux-gnu"
define void @gvnsink(ptr %p, i32 %a, i32 %b, i1 %c) {
entry:
  br i1 %c, label %then, label %else
then:
  %x = add i32 %a, 1
  store volatile i32 %x, ptr %p, align 4
  br label %tail
else:
  %y = add i32 %b, 1
  store volatile i32 %y, ptr %p, align 4
  br label %tail
tail:
  ret void
}
