# BreakCriticalEdges: new uncond `br` on critical edge drops `!annotation` (and `!prof` from MergeIdenticalEdges)

## File and root cause

`llvm/lib/Transforms/Utils/BreakCriticalEdges.cpp:168-180` in
`SplitKnownCriticalEdge`.

```c++
// Create a new basic block, linking it into the CFG.
BasicBlock *NewBB = nullptr;
if (BBName.str() != "")
  NewBB = BasicBlock::Create(TI->getContext(), BBName);
else
  NewBB = BasicBlock::Create(TI->getContext(), TIBB->getName() + "." +
                                                   DestBB->getName() +
                                                   "_crit_edge");
// Create our unconditional branch.
UncondBrInst *NewBI = UncondBrInst::Create(DestBB, NewBB);
NewBI->setDebugLoc(TI->getDebugLoc());
if (auto *LoopMD = TI->getMetadata(LLVMContext::MD_loop))
  NewBI->setMetadata(LLVMContext::MD_loop, LoopMD);
```

`NewBI` (the unconditional branch on the new critical-edge split block)
receives exactly `MD_dbg` and `MD_loop` from the original terminator. Compare
with `llvm/lib/Transforms/Utils/Local.cpp:155-159` for the "fold conditional
to unconditional" case, which propagates `MD_loop`, `MD_dbg`, **and
`MD_annotation`**:

```c++
NewBI->copyMetadata(*BI, {LLVMContext::MD_loop, LLVMContext::MD_dbg,
                          LLVMContext::MD_annotation});
```

So the existing in-tree convention for "new unconditional br replacing/extending
a conditional br" already includes `MD_annotation`; `BreakCriticalEdges`
diverges from that convention.

Beyond `!annotation`, the same code path also fails to propagate any
edge-level `!prof` when `MergeIdenticalEdges` is requested (the LSR caller at
`LoopStrengthReduce.cpp:5910` opts into this). When two switch cases that
point to the same destination get merged into one critical-edge block, the
switch's `!prof` weights array keeps a per-case entry for each merged
predecessor index, so the *probability* per edge is preserved on the switch
side — but the newly inserted unconditional `br` loses any `!unpredictable`
hint that was on the source.

## Reproducer

`x86/candidates/w482-bce-annotation.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @f(i1 %c, i32 %x, i32 %y) {
entry:
  br i1 %c, label %t, label %m, !annotation !0, !prof !1

t:
  %a = add i32 %x, 1
  br label %m

m:
  %p = phi i32 [ %a, %t ], [ %y, %entry ]
  ret i32 %p
}

!0 = !{!"my_annotation"}
!1 = !{!"branch_weights", i32 1, i32 100}
```

### `opt -passes=break-crit-edges -S` actual output

```llvm
entry:
  br i1 %c, label %t, label %entry.m_crit_edge, !prof !0, !annotation !1

entry.m_crit_edge:                                ; preds = %entry
  br label %m            ; <-- NO !annotation

t:
  %a = add i32 %x, 1
  br label %m

m:
  %p = phi i32 [ %a, %t ], [ %y, %entry.m_crit_edge ]
  ret i32 %p
}
```

The `!annotation` on the source `br i1` originally tagged the entire branch
(both edges). After splitting, the conditional `br` in `entry` still has it,
but the newly created edge-block branch `br label %m` does not, even though
it physically represents the **same** edge that was annotated.

## Why this is a regression

* LangRef contract: "`!annotation` metadata should be preserved by optimization
  passes." `BreakCriticalEdges` is an in-tree optimization pass and it does
  not preserve it on the split-out edge.
* `!annotation` is used by sanitizers and code-annotation passes (e.g.,
  `MemorySanitizer`, custom out-of-tree tooling, `controlled-convergence`
  region marking) to identify regions of code after IR transformations.
  Dropping it on a split block means downstream analyses see "no annotation"
  on traffic that physically came through the annotated branch.
* `break-crit-edges` is invoked by core mid-end passes:
  * `LoopStrengthReduce` (`-passes=loop-reduce`, default in `-O2` loop
    pipeline) calls `SplitCriticalEdge` per PHI fix-up.
  * Many SelectionDAG/GlobalISel paths run it during ISel preparation.

So this drop will hit any TU with `!annotation` metadata that hits a critical
edge during `-O2`.

## Fix sketch

Match the in-tree convention in `Local.cpp`. Replace the manual MD_loop
copy with:

```c++
NewBI->copyMetadata(*TI, {LLVMContext::MD_loop, LLVMContext::MD_dbg,
                          LLVMContext::MD_annotation,
                          LLVMContext::MD_unpredictable});
```

(`MD_dbg` is already set explicitly above via `setDebugLoc`; `copyMetadata`
overlays cleanly.)
