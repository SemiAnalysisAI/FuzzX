target triple = "x86_64-unknown-linux-gnu"
define void @f(ptr %p) {
  %p1 = getelementptr i8, ptr %p, i64 0
  %l = load i32, ptr %p1, align 4, !nontemporal !0, !tbaa !1
  %r = or i32 %l, 256              ; sets byte+1 = 1
  store i32 %r, ptr %p1, align 4, !nontemporal !0, !tbaa !1
  ret void
}
!0 = !{i32 1}
!1 = !{!2, !2, i64 0}
!2 = !{!"int", !3}
!3 = !{!"root"}
