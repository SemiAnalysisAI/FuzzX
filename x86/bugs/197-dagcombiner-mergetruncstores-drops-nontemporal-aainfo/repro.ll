target triple = "x86_64-unknown-linux-gnu"
define void @f(i32 %v, ptr %p01) {
  %p1 = getelementptr i8, ptr %p01, i64 1
  %p2 = getelementptr i8, ptr %p01, i64 2
  %p3 = getelementptr i8, ptr %p01, i64 3
  %b0 = trunc i32 %v to i8
  %s1 = lshr i32 %v, 8
  %b1 = trunc i32 %s1 to i8
  %s2 = lshr i32 %v, 16
  %b2 = trunc i32 %s2 to i8
  %s3 = lshr i32 %v, 24
  %b3 = trunc i32 %s3 to i8
  store i8 %b0, ptr %p01, align 1, !nontemporal !0, !tbaa !1
  store i8 %b1, ptr %p1,  align 1, !nontemporal !0, !tbaa !1
  store i8 %b2, ptr %p2,  align 1, !nontemporal !0, !tbaa !1
  store i8 %b3, ptr %p3,  align 1, !nontemporal !0, !tbaa !1
  ret void
}
!0 = !{i32 1}
!1 = !{!2, !2, i64 0}
!2 = !{!"char", !3}
!3 = !{!"root"}
