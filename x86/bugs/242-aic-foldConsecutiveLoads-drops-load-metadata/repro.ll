target triple = "x86_64-unknown-linux-gnu"
define i64 @f(ptr %p) {
  %p2 = getelementptr i8, ptr %p, i64 4
  %a = load i32, ptr %p, align 4, !nontemporal !0, !invariant.load !1, !noundef !1
  %b = load i32, ptr %p2, align 4, !nontemporal !0, !invariant.load !1, !noundef !1
  %az = zext i32 %a to i64
  %bz = zext i32 %b to i64
  %bs = shl i64 %bz, 32
  %r = or i64 %az, %bs
  ret i64 %r
}
!0 = !{i32 1}
!1 = !{}
