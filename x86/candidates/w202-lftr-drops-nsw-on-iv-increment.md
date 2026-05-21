## IndVarSimplify (LFTR): drops `nsw` on the IV increment after rewriting exit cond

**Severity:** Missed optimization at -O2 (poison weakening, not a miscompile).
The downstream optimizer loses a fact SCEV had proven, blocking other folds.

**File:** `llvm/lib/Transforms/Scalar/IndVarSimplify.cpp:1099-1117`
(`linearFunctionTestReplace`).

### What goes wrong

After LFTR rewrites the loop exit condition, it tries to narrow the
nowrap flags on the increment to match what SCEV inferred for the
post-inc addrec:

```cpp
if (auto *BO = dyn_cast<BinaryOperator>(IncVar)) {
  const SCEVAddRecExpr *AR = cast<SCEVAddRecExpr>(SE->getSCEV(IncVar));
  if (BO->hasNoUnsignedWrap())
    BO->setHasNoUnsignedWrap(AR->hasNoUnsignedWrap());
  if (BO->hasNoSignedWrap())
    BO->setHasNoSignedWrap(AR->hasNoSignedWrap());
}
```

If `AR->hasNoSignedWrap()` returns false (which it commonly does after
LFTR shifts the exit-test polarity, because the post-inc addrec ranges
one step further than the pre-inc), the increment loses its `nsw`. The
original `nsw` may have been validly inferred at IR-construction time;
LFTR's bookkeeping conservatively re-asks SCEV and accepts the answer
without trying to prove the post-inc nsw via range analysis.

### Repro (visible at -O2)

```ll
; reducer.ll
target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @use(i32)

define void @lftr_drops_nsw(i32 %n) {
entry:
  %ne = icmp sgt i32 %n, 0
  br i1 %ne, label %loop, label %exit
loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  call void @use(i32 %i)
  %i.next = add nuw nsw i32 %i, 1     ; <-- has both flags in source
  %cmp = icmp slt i32 %i, %n
  br i1 %cmp, label %loop, label %exit
exit:
  ret void
}
```

`opt -O2 -S reducer.ll` (default x86 pipeline):

```ll
loop:
  %i = phi i32 [ %i.next, %loop ], [ 0, %entry ]
  tail call void @use(i32 %i)
  %i.next = add nuw i32 %i, 1          ; <-- nsw dropped
  %exitcond.not = icmp eq i32 %i.next, %n
  br i1 %exitcond.not, label %exit, label %loop
```

### Reasoning about the drop

Original: `i < n` exits when `i >= n`, so `i` ranges 0..n. Computing
`i.next = i + 1` for i = n-1 produces n. For n bounded by INT_MAX,
nsw clearly holds.

After LFTR (post-inc form): `i.next == n` exits. So `i.next` ranges
1..n. SCEV's *post-inc* addrec is `{1,+,1}`. SCEV checks: can adding
1 to (post-inc addrec value) overflow nsw? The maximum value of the
post-inc addrec is n, so `i.next + 1` could be n+1, which when n =
INT_MAX, signed-overflows. So SCEV's `AR->hasNoSignedWrap()` returns
false, and the nsw gets dropped.

But `AR` is the post-inc addrec — i.e., the addrec describing the
*value* of i.next, not the *operation* that computes i.next. The
addrec sequence is 1, 2, ..., n, n+1, ... but the *loop body* only
ever executes when i.next != n, so the actual operation `add i, 1`
that produces i.next never has a poisonous input. The drop is
unnecessary in the common case.

### Why this matters in the -O2 pipeline

Downstream loop passes (LoopVectorize, LoopUnroll, LICM) use the
increment's nsw to prove no overflow during stride and trip-count
math. With nsw dropped, the vectorizer's overflow-aware widening may
be conservative, generating runtime overflow checks the original IR
didn't need.

### Source-author-flagged TODO at line 1107-1110

```
// TODO: This handling is inaccurate for one case: If we switch to a
// dynamically dead IV that wraps on the first loop iteration only,
// which is not covered by the post-inc addrec. (If the new IV was
// not dynamically dead, it could not be poison on the first iteration
// in the first place.)
```

This TODO acknowledges the more serious *correctness* gap (a soundness
hole for "dynamically dead" IVs), while the case shown above is the
benign optimization-loss flavor of the same code.

### Status

Confirmed via `opt -O2 -S` diff: the nsw flag on the IV increment is
stripped. Soundness-relevant TODO present in source. Worth treating as
both a missed-optimization audit point and a soundness review item.
