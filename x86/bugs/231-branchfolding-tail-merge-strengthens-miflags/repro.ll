target triple = "x86_64-unknown-linux-gnu"
declare void @sink(i32)
define void @f(i1 %c, i32 %x, i32 %y) {
entry:
  br i1 %c, label %a, label %b
a:
  %sa = add nuw i32 %x, %y
  call void @sink(i32 %sa)
  br label %end
b:
  %sb = add i32 %x, %y         ; no nuw
  call void @sink(i32 %sb)
  br label %end
end:
  ret void
}
