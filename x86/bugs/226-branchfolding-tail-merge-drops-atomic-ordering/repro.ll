target triple = "x86_64-unknown-linux-gnu"
define i32 @f(i1 %c, ptr %p) {
entry:
  br i1 %c, label %a, label %b
a:
  %la = load atomic i32, ptr %p monotonic, align 4
  br label %end
b:
  %lb = load i32, ptr %p, align 4
  br label %end
end:
  %v = phi i32 [ %la, %a ], [ %lb, %b ]
  ret i32 %v
}
