# w672: JumpThreading `duplicateCondBranchOnPHIIntoPred` clones the cond-branch's `!prof` verbatim into PredBB, no per-edge scaling

## Pass
`-passes=jump-threading` (default x86 -O2 pipeline includes this).

## Summary

`duplicateCondBranchOnPHIIntoPred` duplicates the contents of BB (including
its terminator — a conditional branch on a PHI) into PredBB, then nukes
the unconditional `PredBB -> BB` edge. The terminator is cloned via
`Instruction::clone()`, which **preserves all metadata verbatim**, including
`!prof`. The cloned branch in PredBB is left with the exact same weights
as the original branch in BB.

This is wrong: BB's `!prof` represents the aggregate distribution across
**all** of BB's predecessors. The cloned branch in PredBB represents only
the BB-via-PredBB sub-path, whose distribution may differ wildly. After
the duplication, both branches advertise the same (e.g.) 99/1 split — the
profile information is double-counted and the per-predecessor split has
been lost.

The corresponding BPI update at line 2761 (`BPI->copyEdgeProbabilities(BB,
PredBB)`) propagates the same mistake into the in-memory analysis state,
so the verbatim `!prof` and verbatim BPI agree with each other but neither
is correct for the duplicated path.

## Source (LLVM 23.0.0git, `llvm/lib/Transforms/Scalar/JumpThreading.cpp`)

```cpp
// JumpThreading.cpp:2694-2702 — clones every instruction in BB (including
// the terminator at BB->end()) into PredBB.
for (; BI != BB->end(); ++BI) {
  Instruction *New = BI->clone();         // <-- clone() preserves all MD, incl. !prof
  New->insertInto(PredBB, OldPredBranch->getIterator());
  ...
}
```

and

```cpp
// JumpThreading.cpp:2760-2761
if (auto *BPI = getBPI())
  BPI->copyEdgeProbabilities(BB, PredBB);   // <-- verbatim copy, no per-edge scaling
```

`Instruction::clone()` copies all metadata including `MD_prof`. The cloned
branch in PredBB is therefore left with the original BB-aggregate weights.
There is no scaling by P(this pred is taken).

## Reproducer

Input `final_c.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"

declare void @f1()
declare void @f2()
declare void @f3()
declare i1 @make_bool()

; processBranchOnXOR triggers duplicateCondBranchOnPHIIntoPred, copying the
; CondBr on %x (and its !prof) into pred B.
define void @test(i1 %c) {
entry:
  br i1 %c, label %A, label %B
A:
  call void @f1()
  br label %merge
B:
  call void @f2()
  br label %merge
merge:
  %p = phi i1 [ true, %A ], [ false, %B ]
  %unknown = call i1 @make_bool()
  %x = xor i1 %p, %unknown
  br i1 %x, label %T, label %F, !prof !0
T:
  call void @f3()
  ret void
F:
  call void @f1()
  ret void
}
!0 = !{!"branch_weights", i32 99, i32 1}
```

Command:
```
opt -passes=jump-threading -S final_c.ll
```

Actual output (excerpt):
```llvm
B:                                                ; preds = %entry
  call void @f2()
  %unknown1 = call i1 @make_bool()
  br i1 %unknown1, label %T, label %F, !prof !0      ; <-- cloned !prof, 99/1
merge:                                            ; preds = %entry
  call void @f1()
  %unknown = call i1 @make_bool()
  %x = xor i1 true, %unknown
  br i1 %x, label %T, label %F, !prof !0             ; <-- original !prof, 99/1
```

Two conditional branches now exist, each annotated `!prof !{99,1}`. The
original BB hot edge was T (99/100). If we trust the new metadata
literally, the program now appears to take T with probability ~99% on
*both* paths — i.e. the aggregate is no longer 99% but
(P(entry→merge) · 99 + P(entry→B) · 99) ≈ 99% × everything — which
double-counts entry→B's contribution. Subsequent passes (BlockFrequencyInfo
recompute, register allocator hot/cold heuristics, function layout) all
consume `!prof` and get a distorted view.

Note also that the cloned `xor i1 true, %unknown` was further simplified
inline to `br i1 %unknown1, ...`. The `!prof` is on a *different* condition
now (the original was `xor`, the clone simplifies to `%unknown`), making
the verbatim copy even less defensible — the new branch is not the same
event as the old one.

## Why this matters

- BPI/BFI consumers (CodeGen frame layout, MachineBlockPlacement,
  register allocator splitting) read `!prof` and assume it's calibrated
  for the branch it's attached to. Double-counted weights mislead these
  passes.
- For PGO with very-hot/very-cold splits (e.g. `99/1`), the wrong
  scaling can promote a cold path to hot or vice versa.
- A subsequent JumpThreading or SimplifyCFG can re-thread based on these
  weights and amplify the mistake.

## Suggested fix

Two options, both straightforward:

1. **Strip `!prof` on the clone**: after cloning, drop `MD_prof` from the
   cloned terminator so downstream re-derives it from BPI:
   ```cpp
   if (auto *Br = dyn_cast<BranchInst>(New))
     Br->setMetadata(LLVMContext::MD_prof, nullptr);
   ```
   This is safe (subsequent passes recompute) and matches the spirit of
   "we don't know the per-pred split, so don't lie".

2. **Scale the weights by P(PredBB→BB)**: derive a scale factor from BPI
   and emit scaled weights on both copies. Symmetric to what
   `updateBlockFreqAndEdgeWeight` does for the threadEdge path.

A similar fix is needed in `threadThroughTwoBasicBlocks` (line 2300 —
`cloneInstructions` ends up cloning the conditional terminator into NewBB).
See w673 for that sibling bug.

## Related

- w673 — `threadThroughTwoBasicBlocks` has the same wrong-scaling pattern
  (cloned `PredBB` terminator retains verbatim `!prof`).
