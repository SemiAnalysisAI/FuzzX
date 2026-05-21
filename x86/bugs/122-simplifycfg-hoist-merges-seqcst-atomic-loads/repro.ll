target triple = "x86_64-unknown-linux-gnu"
define i32 @atomic_hoist(ptr %p, i1 %c) {
entry:
  br i1 %c, label %then, label %else
then:
  %a = load atomic i32, ptr %p seq_cst, align 4
  %x = add i32 %a, 1
  br label %tail
else:
  %b = load atomic i32, ptr %p seq_cst, align 4
  %y = add i32 %b, 2
  br label %tail
tail:
  %r = phi i32 [ %x, %then ], [ %y, %else ]
  ret i32 %r
}
