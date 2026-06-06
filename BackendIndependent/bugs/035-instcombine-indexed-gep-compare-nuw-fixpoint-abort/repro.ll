target datalayout = "e-p:32:32:32-i1:8:8-i8:8:8-i16:16:16-i32:32:32-i64:32:32-f32:32:32-f64:32:32-v64:64:64-v128:128:128-a0:0:64"

define ptr @indexed_gep_compare_nuw_fixpoint(ptr %A, i32 %Offset) {
entry:
  %tmp = getelementptr inbounds i32, ptr %A, i32 %Offset
  br label %bb

bb:
  %RHS = phi ptr [ %RHS.next, %bb ], [ %tmp, %entry ]
  %LHS = getelementptr inbounds i32, ptr %A, i32 100
  %RHS.next = getelementptr inbounds i32, ptr %RHS, i64 1
  %cond = icmp ult ptr %LHS, %RHS
  br i1 %cond, label %bb2, label %bb

bb2:
  ret ptr %RHS
}
