target triple = "x86_64-unknown-linux-gnu"
declare i64 @llvm.expect.i64(i64, i64)
define i32 @f(i64 %x) {
entry:
  %e = call i64 @llvm.expect.i64(i64 %x, i64 0)
  %c = icmp ne i64 %e, 0
  br i1 %c, label %t, label %f, !prof !0
t:
  ret i32 1
f:
  ret i32 2
}
!0 = !{!"branch_weights", i32 5000, i32 100}
