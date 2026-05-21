target triple = "x86_64-unknown-linux-gnu"

@vt = constant [16 x ptr] zeroinitializer

define ptr @lcssa_addr_test(i64 %n) {
entry:
  br label %loop

loop:
  %i = phi i64 [ 0, %entry ], [ %i.next, %loop ]
  ; Base = constexpr GEP with inrange. Offset = IV*8.
  %off = shl i64 %i, 3
  %addr = getelementptr inbounds i8, ptr getelementptr inbounds inrange(-8, 24) ([16 x ptr], ptr @vt, i64 0, i64 1), i64 %off
  %i.next = add nuw nsw i64 %i, 1
  %cond = icmp ult i64 %i.next, %n
  br i1 %cond, label %loop, label %exit

exit:
  %ret = phi ptr [ %addr, %loop ]
  ret ptr %ret
}
