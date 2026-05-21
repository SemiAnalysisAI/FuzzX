target triple = "x86_64-unknown-linux-gnu"
declare i32 @get()
define void @f(i32 %n, ptr %p) {
entry:
  %inv = call i32 @get()
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %inext, %tail ]
  switch i32 %inv, label %def [
    i32 0, label %c0
    i32 1, label %c1
  ], !prof !0
c0:
  store i32 0, ptr %p
  br label %tail
c1:
  store i32 1, ptr %p
  br label %tail
def:
  store i32 2, ptr %p
  br label %tail
tail:
  %inext = add i32 %i, 1
  %cmp = icmp slt i32 %inext, %n
  br i1 %cmp, label %loop, label %exit
exit:
  ret void
}
!0 = !{!"branch_weights", !"expected", i32 100, i32 1, i32 1}
