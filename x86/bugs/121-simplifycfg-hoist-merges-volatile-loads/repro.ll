target triple = "x86_64-unknown-linux-gnu"
define i32 @hoist_volatile(ptr %p, i1 %c) {
entry:
  br i1 %c, label %then, label %else
then:
  %a = load volatile i32, ptr %p, align 4
  %x = add i32 %a, 1
  br label %tail
else:
  %b = load volatile i32, ptr %p, align 4
  %y = add i32 %b, 2
  br label %tail
tail:
  %r = phi i32 [ %x, %then ], [ %y, %else ]
  ret i32 %r
}
