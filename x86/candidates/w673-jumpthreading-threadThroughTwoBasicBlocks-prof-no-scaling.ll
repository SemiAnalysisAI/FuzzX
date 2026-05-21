target triple = "x86_64-unknown-linux-gnu"
@a = external global ptr
declare void @f1()
declare void @f2()
declare void @f3()
declare void @f4()

define void @foo(i32 %cond1, i32 %cond2) {
entry:
  %tobool = icmp eq i32 %cond1, 0
  br i1 %tobool, label %bb.cond2, label %bb.f1
bb.f1:
  call void @f1()
  br label %bb.cond2
bb.cond2:
  %ptr = phi ptr [ null, %bb.f1 ], [ @a, %entry ]
  %tobool1 = icmp eq i32 %cond2, 0
  br i1 %tobool1, label %bb.file, label %bb.f2, !prof !0
bb.f2:
  call void @f2()
  br label %exit
bb.file:
  %cmp = icmp eq ptr %ptr, null
  br i1 %cmp, label %bb.f3, label %bb.f4
bb.f3:
  call void @f3()
  br label %exit
bb.f4:
  call void @f4()
  br label %exit
exit:
  ret void
}
!0 = !{!"branch_weights", i32 80, i32 20}
