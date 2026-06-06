define i8 @loop_flag_store_forward_xor_fixpoint(ptr %p, i8 %x, i1 %c) {
entry:
  %flag = alloca i8, align 1
  %cmp = icmp slt i8 %x, -1
  br label %loop

loop:
  %phi = phi i1 [ %cmp, %entry ], [ %c, %loop ]
  %not1 = xor i1 %phi, true
  %or = or i1 %cmp, %not1
  %not2 = xor i1 %or, true
  %ext2 = zext i1 %not2 to i8
  store i8 %ext2, ptr %p, align 1
  store i8 1, ptr %flag, align 1
  %flagv = load i8, ptr %flag, align 1
  %cond = icmp eq i8 %flagv, 0
  br i1 %cond, label %loop, label %exit

exit:
  %not3 = xor i1 %or, true
  %ext3 = zext i1 %not3 to i8
  ret i8 %ext3
}
