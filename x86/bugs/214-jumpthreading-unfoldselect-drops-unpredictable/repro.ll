target triple = "x86_64-unknown-linux-gnu"
declare i32 @foo()
declare i32 @bar()
define i32 @f(i1 %ext, i32 %x) {
entry:
  br i1 %ext, label %t, label %f
t:
  br label %merge
f:
  br label %merge
merge:
  %phi = phi i1 [ true, %t ], [ false, %f ]
  %a = call i32 @foo()
  %b = call i32 @bar()
  %r = select i1 %phi, i32 %a, i32 %b, !unpredictable !0
  ret i32 %r
}
!0 = !{}
