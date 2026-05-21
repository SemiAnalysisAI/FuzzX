# CGP splitBranchCondition writes stale (un-updated) branch_weights on both new branches

File: `llvm/lib/CodeGen/CodeGenPrepare.cpp:9314-9499` (function `splitBranchCondition`).

## Reasoning

`splitBranchCondition` rewrites `br (X and Y), TBB, FBB` (and the `or` form)
into two conditional branches across two basic blocks. To preserve
profile information, the code carefully recomputes new branch-weights
for the two resulting branches:

```cpp
uint64_t TrueWeight, FalseWeight;
if (extractBranchWeights(*Br1, TrueWeight, FalseWeight)) {
  uint64_t NewTrueWeight  = TrueWeight;
  uint64_t NewFalseWeight = TrueWeight + 2 * FalseWeight;   // OR-case Br1
  scaleWeights(NewTrueWeight, NewFalseWeight);
  Br1->setMetadata(LLVMContext::MD_prof,
                   MDBuilder(Br1->getContext())
                       .createBranchWeights(TrueWeight, FalseWeight,
                                            hasBranchWeightOrigin(*Br1)));

  NewTrueWeight  = TrueWeight;
  NewFalseWeight = 2 * FalseWeight;                          // OR-case Br2
  scaleWeights(NewTrueWeight, NewFalseWeight);
  Br2->setMetadata(LLVMContext::MD_prof,
                   MDBuilder(Br2->getContext())
                       .createBranchWeights(TrueWeight, FalseWeight));
}
```

The exact same anti-pattern is repeated in the `And` arm directly below
(lines 9476-9491). In all four `setMetadata` calls, `createBranchWeights`
is invoked with the **original** `(TrueWeight, FalseWeight)` rather than
the freshly computed `(NewTrueWeight, NewFalseWeight)`. The computed
values, after being passed through `scaleWeights`, are immediately
discarded.

Consequence: the two new branches both advertise the **same skew as the
pre-split fused branch**. That violates the comment's stated math: for
example, for the OR-case the correct false-weight of Br1 should be
`TrueWeight + 2*FalseWeight`, dampened from the original. Profile-driven
codegen (block placement, branch hints, register allocation) downstream
of CGP will read these wrong weights and may invert hot/cold decisions,
swap fall-through, or mis-prioritise spilling.

This bug exists upstream in `main` at the same line numbers (confirmed
from a fresh clone), so it is not a fuzzer-induced corruption.

## IR repro (OR case)

Run with:

```
llc -fast-isel -stop-after=codegenprepare -mtriple=x86_64-unknown-linux-gnu repro.ll -o -
```

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @a()
declare void @b()

define void @split_or(i32 %x, i32 %y) {
entry:
  %c1 = icmp eq i32 %x, 0
  %c2 = icmp eq i32 %y, 0
  %or = or i1 %c1, %c2
  br i1 %or, label %t, label %f, !prof !0
t:
  call void @a()
  ret void
f:
  call void @b()
  ret void
}

!0 = !{!"branch_weights", i32 100, i32 7}
```

(splitBranchCondition only runs when FastISel is enabled and JumpIsCheap;
`-fast-isel` is the simplest way to trigger it.)

## Observed wrong outcome

CGP output:

```
  br i1 %c1, label %t, label %entry.cond.split, !prof !0
  ...
entry.cond.split:
  %c2 = icmp eq i32 %y, 0
  br i1 %c2, label %t, label %f, !prof !0
...
!0 = !{!"branch_weights", i32 100, i32 7}
```

Both branches carry `!{i32 100, i32 7}`, identical to the original fused
branch. The correct, dampened weights are:
- Br1 : `branch_weights 100, 114` (= TrueWeight, TrueWeight+2*FalseWeight)
- Br2 : `branch_weights 100,  14` (= TrueWeight, 2*FalseWeight)

(After scaleWeights, they may be reduced by a common factor.)

The `And` form has the identical defect; the expected weights there per
the in-source comment are `(2A+B, B)` for Br1 and `(2A, B)` for Br2.

## Fix sketch

Four sites: pass the freshly computed and scaled weights:

```cpp
.createBranchWeights(NewTrueWeight, NewFalseWeight, ...)
```

instead of the original `(TrueWeight, FalseWeight)`.
