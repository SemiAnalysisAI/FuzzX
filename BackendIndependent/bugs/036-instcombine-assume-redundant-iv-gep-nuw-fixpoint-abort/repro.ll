target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @assume_redundant_iv_gep_nuw_fixpoint(ptr nocapture readonly dereferenceable(8) %x) nounwind uwtable {
entry:
  %0 = load ptr, ptr %x, align 8
  %ptrint = ptrtoint ptr %0 to i64
  %maskedptr = and i64 %ptrint, 31
  %maskcond = icmp eq i64 %maskedptr, 0
  br label %for.body

for.body:
  %indvars.iv = phi i64 [ 0, %entry ], [ %indvars.iv.next.1, %for.body ]
  tail call void @llvm.assume(i1 %maskcond)
  %arrayidx = getelementptr inbounds double, ptr %0, i64 %indvars.iv
  %1 = load double, ptr %arrayidx, align 16
  %add = fadd double %1, 1.000000e+00
  tail call void @llvm.assume(i1 %maskcond)
  %mul = fmul double %add, 2.000000e+00
  store double %mul, ptr %arrayidx, align 16
  %indvars.iv.next = add nuw nsw i64 %indvars.iv, 1
  tail call void @llvm.assume(i1 %maskcond)
  %arrayidx.1 = getelementptr inbounds double, ptr %0, i64 %indvars.iv.next
  %2 = load double, ptr %arrayidx.1, align 8
  %add.1 = fadd double %2, 1.000000e+00
  tail call void @llvm.assume(i1 %maskcond)
  %mul.1 = fmul double %add.1, 2.000000e+00
  store double %mul.1, ptr %arrayidx.1, align 8
  %indvars.iv.next.1 = add nuw nsw i64 %indvars.iv.next, 1
  %exitcond.1 = icmp eq i64 %indvars.iv.next, 1599
  br i1 %exitcond.1, label %for.end, label %for.body

for.end:
  ret void
}

declare void @llvm.assume(i1)
