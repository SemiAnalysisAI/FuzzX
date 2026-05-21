target triple = "x86_64-unknown-linux-gnu"
define void @f(ptr %p01, ptr %d02) {
  %p2 = getelementptr i32, ptr %p01, i64 1
  %d2 = getelementptr i32, ptr %d02, i64 1
  %a = load i32, ptr %p01, align 4, !tbaa !0, !alias.scope !3, !noalias !6
  %b = load i32, ptr %p2,  align 4, !tbaa !0, !alias.scope !3, !noalias !6
  store i32 %a, ptr %d02, align 4, !tbaa !0, !alias.scope !3, !noalias !6
  store i32 %b, ptr %d2,  align 4, !tbaa !0, !alias.scope !3, !noalias !6
  ret void
}
!0 = !{!1, !1, i64 0}
!1 = !{!"int", !2}
!2 = !{!"root"}
!3 = !{!4}
!4 = distinct !{!4, !5}
!5 = distinct !{!5}
!6 = !{!7}
!7 = distinct !{!7, !5}
