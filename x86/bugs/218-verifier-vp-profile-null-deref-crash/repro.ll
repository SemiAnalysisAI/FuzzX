declare void @use()
define i32 @f() {
  call void @use(), !prof !0
  ret i32 0
}
!0 = !{!"VP", i32 0, i64 100, !"oops", i64 50}
