define zeroext i1 @opeq_i17(ptr nocapture readonly %a, ptr nocapture readonly %b) {
entry:
  %0 = load i17, ptr %a, align 4
  %1 = load i17, ptr %b, align 4
  %cmp.i = icmp eq i17 %0, %1
  br i1 %cmp.i, label %land.rhs.i, label %opeq.exit
land.rhs.i:
  %pa = getelementptr inbounds i8, ptr %a, i64 2
  %2 = load i17, ptr %pa, align 1
  %pb = getelementptr inbounds i8, ptr %b, i64 2
  %3 = load i17, ptr %pb, align 1
  %cmp3.i = icmp eq i17 %2, %3
  br label %opeq.exit
opeq.exit:
  %4 = phi i1 [ false, %entry ], [ %cmp3.i, %land.rhs.i ]
  ret i1 %4
}
