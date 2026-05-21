@g1 = global i32 5, align 16
@g2 = global i32 7, align 16
define i32 @sel_drops_noundef(i1 %c) {
  %p = select i1 %c, ptr @g1, ptr @g2
  %v = load i32, ptr %p, align 4, !noundef !0, !nontemporal !1, !invariant.load !2
  %r = add i32 %v, %v
  ret i32 %r
}
!0 = !{}
!1 = !{i32 1}
!2 = !{}
