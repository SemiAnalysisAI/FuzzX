target triple = "x86_64-unknown-linux-gnu"
declare i64 @llvm.expect.i64(i64, i64)
define i32 @sw(i64 %x) {
entry:
  %e = call i64 @llvm.expect.i64(i64 %x, i64 1)
  switch i64 %e, label %def [
    i64 1, label %c1
    i64 2, label %c2
  ], !prof !0
def:
  ret i32 0
c1:
  ret i32 1
c2:
  ret i32 2
}
!0 = !{!"branch_weights", i32 10, i32 500, i32 400}
