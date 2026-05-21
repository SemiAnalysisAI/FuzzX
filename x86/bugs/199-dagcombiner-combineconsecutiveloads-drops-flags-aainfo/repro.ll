target triple = "x86_64-unknown-linux-gnu"
define i64 @f(ptr %p) {
  %p1 = getelementptr i8, ptr %p, i64 4
  %a = load i32, ptr %p, align 4, !nontemporal !0, !invariant.load !1, !tbaa !2
  %b = load i32, ptr %p1, align 4, !nontemporal !0, !invariant.load !1, !tbaa !2
  %az = zext i32 %a to i64
  %bz = zext i32 %b to i64
  %bsh = shl i64 %bz, 32
  %r = or i64 %az, %bsh
  ret i64 %r
}
!0 = !{i32 1}
!1 = !{}
!2 = !{!3, !3, i64 0}
!3 = !{!"int", !4}
!4 = !{!"root"}
