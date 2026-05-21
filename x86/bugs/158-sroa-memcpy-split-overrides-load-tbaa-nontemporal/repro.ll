target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i64 @test(ptr %src) {
entry:
  %a = alloca [16 x i8], align 8
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %a, ptr align 8 %src, i64 16, i1 false), !tbaa !2
  %v0 = load i64, ptr %a,                            align 8, !nontemporal !0, !tbaa !4
  %p1 = getelementptr inbounds i8, ptr %a, i64 8
  %v1 = load i64, ptr %p1,                            align 8, !nontemporal !0, !tbaa !6
  %sum = add i64 %v0, %v1
  ret i64 %sum
}
declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)
!0 = !{i32 1}
!1 = !{!"root"}
!2 = !{!3, !3, i64 0}
!3 = !{!"any pointer", !1, i64 0}
!4 = !{!5, !5, i64 0}
!5 = !{!"int", !1, i64 0}
!6 = !{!7, !7, i64 0}
!7 = !{!"float", !1, i64 0}
