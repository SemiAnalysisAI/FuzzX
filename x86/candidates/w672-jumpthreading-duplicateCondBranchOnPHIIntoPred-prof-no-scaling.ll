target triple = "x86_64-unknown-linux-gnu"

declare void @f1()
declare void @f2()
declare void @f3()
declare i1 @make_bool()

define void @test(i1 %c) {
entry:
  br i1 %c, label %A, label %B
A:
  call void @f1()
  br label %merge
B:
  call void @f2()
  br label %merge
merge:
  %p = phi i1 [ true, %A ], [ false, %B ]
  %unknown = call i1 @make_bool()
  %x = xor i1 %p, %unknown
  br i1 %x, label %T, label %F, !prof !0
T:
  call void @f3()
  ret void
F:
  call void @f1()
  ret void
}
!0 = !{!"branch_weights", i32 99, i32 1}
