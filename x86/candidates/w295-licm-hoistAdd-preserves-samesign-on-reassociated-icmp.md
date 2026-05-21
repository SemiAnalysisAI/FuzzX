# w295 -- LICM `hoistAdd` keeps `samesign` flag on reassociated ICmp -> poison-introducing miscompile

## Component
`llvm/lib/Transforms/Scalar/LICM.cpp`, `hoistAdd` at lines 2557-2610.

The reassociation that rewrites the predicate in place:

```cpp
IRBuilder<> Builder(Preheader->getTerminator());
Value *NewCmpOp =
    Builder.CreateSub(InvariantRHS, InvariantOp, "invariant.op",
                      /*HasNUW*/ !IsSigned, /*HasNSW*/ IsSigned);
ICmp.setPredicate(Pred);
ICmp.setOperand(0, VariantOp);
ICmp.setOperand(1, NewCmpOp);
```
(LICM.cpp:2596-2604)

`ICmpInst::setPredicate(Predicate)` only updates the predicate enum; it
does NOT touch the per-instruction `samesign` flag carried by the
`PossiblySameSignInst` subclass. So if the original `%cmp` was
`icmp samesign slt %sum, C2` and we reassociate to
`icmp samesign slt %i, (C2 - C1)`, the flag survives unchanged onto a
comparison whose two operands no longer have the same-sign relationship
that was implied originally.

Per LangRef ("if the samesign keyword is present and the operands are
not of the same sign then the result is a poison value"), this turns a
previously well-defined comparison into poison for some inputs --
a miscompile.

## Root cause
The transform `(LV + C1) cmp C2  ==>  LV cmp (C2 - C1)` changes the LHS
of the icmp from `LV + C1` to `LV`. The `samesign` flag asserts
`sign(LHS) == sign(RHS)`. The relationship `sign(LV + C1) == sign(C2)`
does not imply `sign(LV) == sign(C2 - C1)`. Concretely, for the
sample inputs below, the original `LV + C1` and `C2` have matching
signs (both non-negative), but `LV` itself has the opposite sign from
`C2 - C1`, so the new comparison is poison while the original was a
defined Boolean.

`hoistAdd` must `ICmp.dropPoisonGeneratingFlags()` (or specifically
clear `samesign` via `setSameSign(false)`) when reassociating, the same
way `hoistMulAddAssociation` already calls `dropPoisonGeneratingFlags`
on the intermediate adds (LICM.cpp:2792-2795).

## Reproducer
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @sink(i1)

define void @samesign_hoistadd(i32 %n) {
entry:
  br label %loop

loop:
  %i   = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %sum = add nsw i32 %i, 5
  ; samesign holds for %i in [-5, INT_MAX-5] (sum>=0 matches sign(100))
  %cmp = icmp samesign slt i32 %sum, 100
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
define void @samesign_hoistadd(i32 %n) {
entry:
  br label %loop

loop:                                             ; preds = %loop, %entry
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %cmp = icmp samesign slt i32 %i, 95               ; <-- samesign kept!
  call void @sink(i1 %cmp)
  %i.next = add nsw i32 %i, 1
  %loop.cmp = icmp slt i32 %i.next, %n
  br i1 %loop.cmp, label %loop, label %exit

exit:                                             ; preds = %loop
  ret void
}
```

## Why it's a miscompile (not refinement)
For `%i = -3`:
- Source: `%sum = -3 + 5 = 2`. `samesign slt(2, 100)`. `sign(2) == sign(100)`
  (both non-negative). Flag holds. Result: `2 < 100 == true`.
- After LICM: `samesign slt(-3, 95)`. `sign(-3) != sign(95)`. Flag violated.
  Result: **poison**.

A previously-defined `true` is replaced by poison. This is the textbook
"target more poisonous than source" refinement violation (see also
issue #120361 for the same root-cause pattern fixed in InstCombine).

Downstream uses of `%cmp` (e.g. as a branch condition, or feeding a
`select` whose selected value is itself defined) now exhibit undefined
behaviour for inputs that were perfectly well-defined in the source.

## Default-pipeline reachability
`-passes='loop-mssa(licm)'` reproduces with stock LICM; `-O2` runs LICM
in the standard loop pipeline. The reassociation has no opt-in flag --
it fires whenever the overflow predicate for `InvariantRHS - InvariantOp`
is `NeverOverflows`, which is always true here (`100 - 5` does not
overflow a signed i32).
