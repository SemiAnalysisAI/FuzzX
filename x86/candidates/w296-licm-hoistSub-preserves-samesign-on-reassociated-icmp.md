# w296 -- LICM `hoistSub` keeps `samesign` flag on reassociated ICmp -> poison-introducing miscompile

## Component
`llvm/lib/Transforms/Scalar/LICM.cpp`, `hoistSub` at lines 2615-2692.

The reassociation, mirroring `hoistAdd`:

```cpp
Value *NewCmpOp =
    VariantSubtracted
        ? Builder.CreateSub(InvariantOp, InvariantRHS, "invariant.op",
                            /*HasNUW*/ !IsSigned, /*HasNSW*/ IsSigned)
        : Builder.CreateAdd(InvariantOp, InvariantRHS, "invariant.op",
                            /*HasNUW*/ !IsSigned, /*HasNSW*/ IsSigned);
ICmp.setPredicate(Pred);
ICmp.setOperand(0, VariantOp);
ICmp.setOperand(1, NewCmpOp);
```
(LICM.cpp:2677-2685)

Just like w295's `hoistAdd`, the call to `ICmp.setPredicate(Pred)` only
mutates the predicate enum -- it does not clear the `samesign` flag on
the `PossiblySameSignInst`. The two operands of the icmp are replaced,
but the same-sign assertion about the *old* pair is implicitly carried
over to the *new* pair.

## Root cause
The transform handles three patterns; the broken one is
`(LV - C1) cmp C2  ==>  LV cmp (C1 + C2)` (the `!VariantSubtracted`
branch). The new RHS is `C1 + C2`, but `sign(LV - C1) == sign(C2)` does
NOT imply `sign(LV) == sign(C1 + C2)`. Picking `C2 < 0` and `C1 > 0`
small enough produces inputs where the original samesign predicate
holds and the new one is poison.

Fix: same as w295 -- the rewritten icmp must drop `samesign` (or call
`ICmp.dropPoisonGeneratingFlags()`) before being reused.

## Reproducer
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @sink(i1)

define void @samesign_hoistsub(i32 %n) {
entry:
  br label %loop

loop:
  %i    = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %diff = sub nsw i32 %i, 5
  %cmp  = icmp samesign slt i32 %diff, -100   ; samesign holds whenever diff<0
  call void @sink(i1 %cmp)
  %i.next   = add nsw i32 %i, 1
  %loop.cmp = icmp slt i32 %i.next, %n
  br i1 %loop.cmp, label %loop, label %exit

exit:
  ret void
}
```

`opt -passes='loop-mssa(licm)' -S` (LLVM 23.0.0git x86):
```ll
define void @samesign_hoistsub(i32 %n) {
entry:
  br label %loop

loop:                                             ; preds = %loop, %entry
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %cmp = icmp samesign slt i32 %i, -95              ; <-- samesign kept!
  call void @sink(i1 %cmp)
  %i.next = add nsw i32 %i, 1
  %loop.cmp = icmp slt i32 %i.next, %n
  br i1 %loop.cmp, label %loop, label %exit

exit:                                             ; preds = %loop
  ret void
}
```

## Why it's a miscompile
For `%i = 4`:
- Source: `%diff = 4 - 5 = -1`. `samesign slt(-1, -100)`. `sign(-1) == sign(-100)`
  (both negative). Flag holds. Result: `-1 < -100 == false`.
- After LICM: `samesign slt(4, -95)`. `sign(4) != sign(-95)`. Flag violated.
  Result: **poison**.

A defined `false` becomes poison -- target is strictly more poisonous than
source, identical refinement-violation pattern to w295.

## Default-pipeline reachability
Standard `loop-mssa(licm)` reproduces. No opt-in needed; `hoistSub`
fires whenever `computeOverflowForSigned/UnsignedAdd(InvariantOp, InvariantRHS, ...)`
returns `NeverOverflows`, which is trivially true for the constant
operands here.
