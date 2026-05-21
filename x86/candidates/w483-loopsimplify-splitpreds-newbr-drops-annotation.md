# LoopSimplify (via `SplitBlockPredecessors`) drops `!annotation` on new preheader / dedicated-exit branches

## File and root cause

`llvm/lib/Transforms/Utils/BasicBlockUtils.cpp:1300-1322` —
`SplitBlockPredecessorsImpl` creates the new block's terminator:

```c++
// Create new basic block, insert right before the original block.
BasicBlock *NewBB = BasicBlock::Create(
    BB->getContext(), BB->getName() + Suffix, BB->getParent(), BB);

// The new block unconditionally branches to the old block.
UncondBrInst *BI = UncondBrInst::Create(BB, NewBB);

Loop *L = nullptr;
BasicBlock *OldLatch = nullptr;
// Splitting the predecessors of a loop header creates a preheader block.
if (LI && LI->isLoopHeader(BB)) {
  L = LI->getLoopFor(BB);
  // Using the loop start line number prevents debuggers stepping into the
  // loop body for this instruction.
  BI->setDebugLoc(L->getStartLoc());
  ...
} else
  BI->setDebugLoc(BB->getFirstNonPHIOrDbg()->getDebugLoc());
```

`BI` (the new uncond branch) gets only a `DebugLoc`. **No metadata transfer
at all.** The predecessors' branches (which originally targeted `BB` and now
target `NewBB`) keep their `!annotation` / `!prof` / `!unpredictable` — those
edges are now "predecessor → NewBB" — but the newly created edge
"NewBB → BB" carries no metadata. From a source-tracking perspective, that
edge is *the continuation of* the originally annotated edge.

Both LoopSimplify entry points hit this:

* `LoopSimplify.cpp:137` — `InsertPreheaderForLoop` → `SplitBlockPredecessors`
* `LoopSimplify.cpp:94` (in `formDedicatedExitBlocks`, defined at
  `LoopUtils.cpp:94`) → `SplitBlockPredecessors`

## Reproducer

`x86/candidates/w483-loopsimplify-annot.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @f(i32 %n, i1 %p, i32 %m) {
entry:
  br i1 %p, label %h, label %exit, !annotation !2

h:
  %i = phi i32 [0, %entry], [%inc, %lat]
  %c = icmp slt i32 %i, %n
  br i1 %c, label %lat, label %exit, !annotation !3

lat:
  %inc = add i32 %i, 1
  br label %h

exit:
  %r = phi i32 [%m, %entry], [%i, %h]
  ret i32 %r
}

!2 = !{!"entry_annot"}
!3 = !{!"loop_exit_annot"}
```

### `opt -passes=loop-simplify -S` actual output

```llvm
entry:
  br i1 %p, label %h.preheader, label %exit, !annotation !0

h.preheader:                                      ; preds = %entry
  br label %h          ; <-- preheader created, no !annotation

h:
  %i = phi i32 [ %inc, %lat ], [ 0, %h.preheader ]
  %c = icmp slt i32 %i, %n
  br i1 %c, label %lat, label %exit.loopexit, !annotation !1

lat:
  %inc = add i32 %i, 1
  br label %h

exit.loopexit:                                    ; preds = %h
  br label %exit       ; <-- new dedicated-exit block, no !annotation

exit:
  %r = phi i32 [ %m, %entry ], [ %i, %exit.loopexit ]
  ret i32 %r
}

!0 = !{!"entry_annot"}
!1 = !{!"loop_exit_annot"}
```

Both newly inserted unconditional branches (`h.preheader -> h` and
`exit.loopexit -> exit`) lack `!annotation`, even though each is the second
half of an originally annotated edge.

## Why this is a regression

* `loop-simplify` is part of the default `-O2` pipeline (it runs as a
  canonicalization step before most loop passes — LoopRotate, LICM,
  LoopUnswitch, LoopVectorize). Almost any annotated TU containing a loop
  will hit this.
* The same `SplitBlockPredecessorsImpl` path is reused by **many** other
  transforms (jump threading, SimplifyCFG's predecessor splitting, GVN,
  LoopUnroll exit splitting, …), so this root-cause hit blast radius extends
  well beyond just LoopSimplify.
* Effect: any post-pass annotation aggregator (sanitizers, custom
  region-cover tracking, downstream BPI when annotation is used to seed
  branch weights via `!annotation` plugins) sees mismatched/missing
  annotation coverage after canonicalization.

## Fix sketch

In `SplitBlockPredecessorsImpl`, after setting `DebugLoc`, copy the
annotation MD from a representative predecessor's terminator (if all
predecessors-to-be-split agree on it):

```c++
if (!Preds.empty()) {
  Instruction *FirstTI = Preds.front()->getTerminator();
  if (MDNode *Ann = FirstTI->getMetadata(LLVMContext::MD_annotation))
    if (all_of(Preds.drop_front(), [&](BasicBlock *P) {
          return P->getTerminator()->getMetadata(LLVMContext::MD_annotation) ==
                 Ann;
        }))
      BI->setMetadata(LLVMContext::MD_annotation, Ann);
}
```

For preheader insertion (a loop header has a single "external" semantic
entry), copy from the first non-loop predecessor's branch.

A more thorough fix would also propagate `MD_unpredictable` symmetrically.
