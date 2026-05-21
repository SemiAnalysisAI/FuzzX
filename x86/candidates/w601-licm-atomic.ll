target triple = "x86_64-unknown-linux-gnu"

@xx = external global i64

; Two uses of the invariant load (to prevent fold) but no call inside loop.

define i64 @licm_inv_load_after_atomic(ptr noalias %sync, ptr noalias dereferenceable(8) %p, i32 %n) {
entry:
  br label %loop

loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %acc = phi i64 [ 0, %entry ], [ %acc.next, %loop ]
  %sync.v = load atomic i64, ptr %sync seq_cst, align 8
  %x = load i64, ptr %p, align 8, !invariant.load !0
  store volatile i64 %x, ptr @xx, align 8
  %t = add i64 %sync.v, %x
  %acc.next = add i64 %acc, %t
  %i.next = add i32 %i, 1
  %cmp = icmp eq i32 %i.next, %n
  br i1 %cmp, label %exit, label %loop

exit:
  ret i64 %acc.next
}

!0 = !{}
