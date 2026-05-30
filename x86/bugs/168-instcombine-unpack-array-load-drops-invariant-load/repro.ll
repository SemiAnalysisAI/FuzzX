target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define [2 x i32] @arr_load_inv(ptr %p) {
  %v = load [2 x i32], ptr %p, align 4, !invariant.load !0, !tbaa !1
  ret [2 x i32] %v
}
!0 = !{}
!1 = !{!2, !2, i64 0}
!2 = !{!"int", !3, i64 0}
!3 = !{!"omnipotent char"}
