target triple = "x86_64-unknown-linux-gnu"

@x = external global i32
@y = external global i32

declare void @f()
declare void @g()

define i32 @pre_drops_metadata(i1 %cond) {
  br i1 %cond, label %A, label %B
A:
  store i32 0, ptr @x
  br label %C
B:
  br label %C
C:
  %ptr = phi ptr [@x, %A], [@y, %B]
  %a = load i32, ptr %ptr, align 8, !range !0, !nontemporal !1, !invariant.load !1, !invariant.group !1, !noundef !1, !mmra !2
  %cond2 = icmp eq i32 %a, 0
  br i1 %cond2, label %YES, label %NO
YES:
  call void @f()
  ret i32 %a
NO:
  call void @g()
  ret i32 1
}
!0 = !{i32 0, i32 100}
!1 = !{}
!2 = !{!"foo", !"bar"}
