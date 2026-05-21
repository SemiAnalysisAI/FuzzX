target triple = "x86_64-unknown-linux-gnu"
@const_pool = constant <2 x i64> <i64 1, i64 2>
define <2 x i64> @f(i1 %c, ptr %p) {
entry:
  br i1 %c, label %t, label %f
t:
  %a = load <2 x i64>, ptr @const_pool, align 16, !invariant.load !0
  br label %end
f:
  %b = load <2 x i64>, ptr @const_pool, align 16, !invariant.load !0, !nontemporal !1
  br label %end
end:
  %v = phi <2 x i64> [ %a, %t ], [ %b, %f ]
  ret <2 x i64> %v
}
!0 = !{}
!1 = !{i32 1}
