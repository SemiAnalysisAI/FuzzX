# w648 - SimplifyCFG `performBranchToCommonDestFolding` drops `!unpredictable` from BI when folding into PBI

## Location

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp`

- `performBranchToCommonDestFolding` lines 3949-4060.
- The metadata propagation block at lines 4028-4031 only forwards `MD_loop`
  and only collects `MD_annotation` via `Builder.CollectMetadataToCopy`
  (line 3969) for the *bonus instructions* — not for the merged condition
  branch.
- The new condition is installed at line 4048
  `PBI->setCondition(createLogicalOp(...));` and the surviving terminator
  is PBI, which keeps PBI's metadata and silently loses BI's
  `!unpredictable`.

Excerpt:

```cpp
// If BI was a loop latch, it may have had associated loop metadata.
// We need to copy it to the new latch, that is, PBI.
if (MDNode *LoopMD = BI->getMetadata(LLVMContext::MD_loop))
  PBI->setMetadata(LLVMContext::MD_loop, LoopMD);

ValueToValueMapTy VMap; // maps original values to cloned values
cloneInstructionsIntoPredecessorBlockAndUpdateSSAUses(BB, PredBlock, VMap);

...

// Now that the Cond was cloned into the predecessor basic block,
// or/and the two conditions together.
Value *BICond = VMap[BI->getCondition()];
PBI->setCondition(
    createLogicalOp(Builder, Opc, PBI->getCondition(), BICond, "or.cond"));
```

The reachable destinations of PBI now depend on BI's (predicate) condition
too, so any unpredictability that BI declared about that condition should
attach to PBI — but it doesn't.

This is reached from `SimplifyCFGOpt::simplifyCondBranch` at line 8605
under `Options.SpeculateBlocks` (default true) and `BonusInstThreshold`
(default 1), so it fires in the default pass-spec.

## Repro (`repro.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

declare void @use(i32)

define void @fbcd_unpred(i1 %c1, i1 %c2) {
entry:
  br i1 %c1, label %check, label %miss, !prof !1
check:
  br i1 %c2, label %hit, label %miss, !unpredictable !0
hit:
  call void @use(i32 1)
  call void @use(i32 1)
  call void @use(i32 1)
  ret void
miss:
  call void @use(i32 0)
  call void @use(i32 0)
  call void @use(i32 0)
  ret void
}

!0 = !{}
!1 = !{!"branch_weights", i32 100, i32 1}
```

## Invocation

```
opt -passes=simplifycfg -S repro.ll
```

## Observed output

```
define void @fbcd_unpred(i1 %c1, i1 %c2) {
entry:
  %c1.not = xor i1 %c1, true
  %c2.not = xor i1 %c2, true
  %brmerge = select i1 %c1.not, i1 true, i1 %c2.not, !prof !0
  br i1 %brmerge, label %miss, label %hit, !prof !1     ; <-- no !unpredictable

...
}

!0 = !{!"branch_weights", i32 1, i32 100}
!1 = !{!"branch_weights", i32 102, i32 100}
```

The post-fold branch in `entry` is the disjunction of `!c1` (from the
predecessor `!prof` branch) and `!c2` (the BI that had `!unpredictable`).
Branch weights for both have been combined into `!1`. But the original
`!unpredictable` from the inner BI is gone, even though that
unpredictability now controls part of the entry branch.

## Why this is a real regression and not "just" missing-MD propagation

`!unpredictable` is acted on by:

- `BranchProbabilityInfo::calcUnpredictableBranchHeuristics` —
  unpredictable branches are treated as 50/50 and exempt from the heuristic
  estimators (loop-exit, pointer-NULL, integer-zero, ...).
- `SimplifyCFGOpt::shouldFoldCondBranchesToCommonDestination` itself
  (line 3921) — it skips the fold when PBI has `!unpredictable`. Losing it
  on PBI can enable a chain of *further* folds on the merged predicate
  that the original IR would have refused.
- `foldTwoEntryPHINode` (line 3713) — speculation decisions key off
  `!unpredictable` on the dominator branch.

So dropping `!unpredictable` can cascade into additional speculative
folds that the user explicitly opted out of.

## Fix

In `performBranchToCommonDestFolding`, after computing the combined
condition, propagate the unpredictability:

```cpp
if (BI->getMetadata(LLVMContext::MD_unpredictable) ||
    PBI->getMetadata(LLVMContext::MD_unpredictable))
  PBI->setMetadata(LLVMContext::MD_unpredictable,
                   MDNode::get(PBI->getContext(), {}));
```

The "either pred ⇒ merged is unpredictable" rule is the right
conservative semantics: a disjunction/conjunction of an unpredictable
condition with anything else is itself unpredictable.

Same fix should be considered in the `InvertBranch` callsite at line
3974, but `InvertBranch` should already preserve metadata of the same
branch (the issue is purely the cross-branch merging that follows).
