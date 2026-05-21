# w673: JumpThreading `threadThroughTwoBasicBlocks` clones PredBB's terminator (with `!prof`) into NewBB, no per-path scaling

## Pass
`-passes=jump-threading` (default x86 -O2 pipeline includes this).

## Summary

`threadThroughTwoBasicBlocks` duplicates PredBB into `NewBB` to thread a
two-block edge `PredPredBB -> PredBB -> BB -> SuccBB`. The duplication is
done via `cloneInstructions(ValueMapping, PredBB->begin(), PredBB->end(),
NewBB, PredPredBB)`. The end-iterator includes PredBB's **terminator** —
the conditional branch `PredBBBranch` — and `Instruction::clone()` copies
all metadata verbatim, including `!prof`.

This is wrong for exactly the same reason as w672: PredBB's `!prof`
represents the aggregate branch distribution across all preds of PredBB,
not the distribution for the specific PredPredBB→PredBB sub-path that
NewBB now represents. After the clone both PredBB and NewBB advertise the
same weights — the per-path information is lost and the aggregate is
double-counted.

The accompanying BPI update at line 2305 (`BPI->copyEdgeProbabilities(
PredBB, NewBB)`) propagates the same mistake into BPI.

## Source (LLVM 23.0.0git, `llvm/lib/Transforms/Scalar/JumpThreading.cpp`)

```cpp
// JumpThreading.cpp:2298-2305
// We are going to have to map operands from the original BB block to the new
// copy of the block 'NewBB'.  If there are PHI nodes in PredBB, evaluate them
// to account for entry from PredPredBB.
ValueToValueMapTy ValueMapping;
cloneInstructions(ValueMapping, PredBB->begin(), PredBB->end(), NewBB,
                  PredPredBB);                              // <-- clones terminator too

// Copy the edge probabilities from PredBB to NewBB.
if (BPI)
  BPI->copyEdgeProbabilities(PredBB, NewBB);                // <-- verbatim copy
```

`cloneInstructions` (line 2094) does `Instruction *New = BI->clone();` which
preserves `!prof` on the cloned conditional branch.

## Reproducer

Input `final_d.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"
@a = external global ptr
declare void @f1()
declare void @f2()
declare void @f3()
declare void @f4()

define void @foo(i32 %cond1, i32 %cond2) {
entry:
  %tobool = icmp eq i32 %cond1, 0
  br i1 %tobool, label %bb.cond2, label %bb.f1
bb.f1:
  call void @f1()
  br label %bb.cond2
bb.cond2:
  %ptr = phi ptr [ null, %bb.f1 ], [ @a, %entry ]
  %tobool1 = icmp eq i32 %cond2, 0
  br i1 %tobool1, label %bb.file, label %bb.f2, !prof !0
bb.f2:
  call void @f2()
  br label %exit
bb.file:
  %cmp = icmp eq ptr %ptr, null
  br i1 %cmp, label %bb.f3, label %bb.f4
bb.f3:
  call void @f3()
  br label %exit
bb.f4:
  call void @f4()
  br label %exit
exit:
  ret void
}
!0 = !{!"branch_weights", i32 80, i32 20}
```

Command:
```
opt -passes=jump-threading -S final_d.ll
```

Actual output (excerpt):
```llvm
bb.cond2:                                         ; preds = %entry
  call void @f1()
  %tobool1 = icmp eq i32 %cond2, 0
  br i1 %tobool1, label %bb.f3, label %bb.f2, !prof !0       ; <-- cloned !prof

bb.cond2.thread:                                  ; preds = %entry
  %tobool12 = icmp eq i32 %cond2, 0
  br i1 %tobool12, label %bb.f4, label %bb.f2, !prof !0      ; <-- verbatim !prof
```

Both branches carry `!prof !0` ⇒ `{80, 20}`. But they branch on the
**same** `%cond2`, after the entry-side split, so their *actual*
distributions are independent draws from the same Bernoulli — yet both
advertise the original aggregate 80/20.

A subsequent BlockFrequency recompute will assume each branch independently
follows 80/20, leading to grossly inflated estimates for the hot edge.

## Why this matters

Same downstream impact as w672:

- CodeGen layout heuristics (MachineBlockPlacement) consume `!prof` for
  block ordering. Duplicate-weighted branches mislead it.
- Register allocator splitting and tail-duplication thresholds consume
  `!prof` — wrong weights → wrong split decisions.
- Iterative JumpThreading inside the same pipeline can compound the error.

## Suggested fix

In `cloneInstructions` (or in `threadThroughTwoBasicBlocks` immediately
after the call), drop `MD_prof` on the cloned conditional terminator (and
also on the original PredBBBranch, since the path-distribution has changed
for both). Alternatively, scale both branches by the analytic
P(PredPredBB → PredBB) / P(any-pred → PredBB) factor.

The minimal safe fix is the strip:

```cpp
// after cloneInstructions(...) in threadThroughTwoBasicBlocks
if (auto *NewBr = dyn_cast<BranchInst>(NewBB->getTerminator()))
  NewBr->setMetadata(LLVMContext::MD_prof, nullptr);
// and on the surviving original:
if (auto *OrigBr = dyn_cast<BranchInst>(PredBB->getTerminator()))
  OrigBr->setMetadata(LLVMContext::MD_prof, nullptr);
```

(BPI is recomputed by callers on demand; stripping IR metadata leaves
later passes free to re-derive weights.)

## Related

- w672 — same bug class in `duplicateCondBranchOnPHIIntoPred`.
