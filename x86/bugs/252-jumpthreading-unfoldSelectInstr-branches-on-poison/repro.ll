target triple = "x86_64-unknown-linux-gnu"
declare void @sink(i32)

define void @test(i1 %c, i1 %maybe_poison) {
entry:
  br i1 %c, label %pred1, label %pred2
pred1:
  %s = select i1 %maybe_poison, i32 0, i32 10
  br label %merge
pred2:
  br label %merge
merge:
  %p = phi i32 [ %s, %pred1 ], [ 5, %pred2 ]
  %cmp = icmp eq i32 %p, 0
  %fcmp = freeze i1 %cmp
  br i1 %fcmp, label %if_then, label %if_else
if_then:
  call void @sink(i32 1)
  ret void
if_else:
  call void @sink(i32 2)
  ret void
}
