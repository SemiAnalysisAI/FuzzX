target triple = "x86_64-unknown-linux-gnu"
define ptr @f(i1 %c, ptr %p) {
entry:
  br i1 %c, label %then, label %merge
then:
  br label %merge
merge:
  %v = load ptr, ptr %p, align 8, !nonnull !0, !dereferenceable !1, !align !2, !noundef !0, !nontemporal !3
  ret ptr %v
}
!0 = !{}
!1 = !{i64 16}
!2 = !{i64 8}
!3 = !{i32 1}
