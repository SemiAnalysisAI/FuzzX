target triple = "x86_64-unknown-linux-gnu"
define void @f(i1 %p, i1 %q, ptr %addr, i32 %pv, i32 %qv) {
entry:
  br i1 %p, label %p.t, label %p.f
p.t:
  store i32 %pv, ptr %addr, align 4, !nontemporal !0, !tbaa !1
  br label %p.merge
p.f:
  br label %p.merge
p.merge:
  br i1 %q, label %q.t, label %end
q.t:
  store i32 %qv, ptr %addr, align 4
  br label %end
end:
  ret void
}
!0 = !{i32 1}
!1 = !{!2, !2, i64 0}
!2 = !{!"int", !3}
!3 = !{!"root"}
