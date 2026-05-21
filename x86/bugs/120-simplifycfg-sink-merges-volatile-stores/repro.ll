target triple = "x86_64-unknown-linux-gnu"
define void @sink_volatile(ptr %p, i32 %a, i32 %b, i1 %c) {
entry:
  br i1 %c, label %then, label %else
then:
  store volatile i32 %a, ptr %p, align 4
  br label %tail
else:
  store volatile i32 %b, ptr %p, align 4
  br label %tail
tail:
  ret void
}
