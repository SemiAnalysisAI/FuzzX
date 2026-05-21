# w671: JumpThreading `unfoldSelectInstr` creates `br i1 SI->getCondition()` without freeze, introducing UB on poison

## Pass
`-passes=jump-threading` (default x86 -O2 pipeline includes this).

## Summary

`unfoldSelectInstr` (called from `tryToUnfoldSelect(CmpInst*)` and
`tryToUnfoldSelect(SwitchInst*)`) expands a select in a predecessor block
into a CFG diamond. The new conditional branch is constructed as:

```cpp
auto *BI = CondBrInst::Create(SI->getCondition(), NewBB, BB, Pred);
```

— directly on `SI->getCondition()`, with no `freeze`. This is a
**soundness regression**: `select i1 poison, ...` is well-defined (the
result is poison, which is fine semantically), but
`br i1 poison` is *immediate undefined behavior*.

If the select's result was eventually consumed by a `freeze` (or any other
poison-blocking sink), the original program was well-defined for all
inputs; after the transform, calling the function with a poison
`SI->getCondition()` is UB.

This is the same class of bug as the well-known requirement to freeze
conditions when "speculatively" turning a select into a branch (compare
`SimplifyCFG`'s `FoldCondBranchOnPHI` and JT's own sibling
`tryToUnfoldSelectInCurrBB` at line 2986-2989 which **does** insert a
freeze when the condition is not guaranteed non-poison).

## Source (LLVM 23.0.0git, `llvm/lib/Transforms/Scalar/JumpThreading.cpp`)

The buggy site:

```cpp
// JumpThreading.cpp:2785-2794
UncondBrInst *PredTerm = cast<UncondBrInst>(Pred->getTerminator());
BasicBlock *NewBB = BasicBlock::Create(BB->getContext(), "select.unfold",
                                       BB->getParent(), BB);
// Move the unconditional branch to NewBB.
PredTerm->removeFromParent();
PredTerm->insertInto(NewBB, NewBB->end());
// Create a conditional branch and update PHI nodes.
auto *BI = CondBrInst::Create(SI->getCondition(), NewBB, BB, Pred);   // <-- no freeze
BI->applyMergedLocation(PredTerm->getDebugLoc(), SI->getDebugLoc());
BI->copyMetadata(*SI, {LLVMContext::MD_prof});
```

For contrast, the sibling routine in the same file does it right:

```cpp
// JumpThreading.cpp:2985-2989
Value *Cond = SI->getCondition();
if (!isGuaranteedNotToBeUndefOrPoison(Cond, nullptr, SI)) {
  Cond = new FreezeInst(Cond, "cond.fr", SI->getIterator());
  cast<FreezeInst>(Cond)->setDebugLoc(DebugLoc::getTemporary());
}
```

The `isGuaranteedNotToBeUndefOrPoison`-guarded freeze is missing entirely
from `unfoldSelectInstr`.

## Reproducer

Input `final_b.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"
declare void @sink(i32)

; Original: %s = select %maybe_poison, 0, 10. select on poison yields poison.
; The poison flows through %p into the icmp -> freeze -> branch. The freeze
; ensures the branch is well-defined regardless of %maybe_poison.
;
; JT applies tryToUnfoldSelect(CmpInst*) -> unfoldSelectInstr, which inserts
;   br i1 %maybe_poison, ...
; directly into pred1. If the caller passes poison for %maybe_poison, the
; original program was well-defined; after JT it is UB.
define void @test(i1 %c, i1 %maybe_poison) {
entry:
  br i1 %c, label %pred1, label %pred2
pred1:
  %s = select i1 %maybe_poison, i32 0, i32 10
  br label %merge
pred2:
  br label %merge
merge:
  %p = phi i32 [ %s, %pred1 ], [ 5, %pred2 ]
  %cmp = icmp eq i32 %p, 0
  %fcmp = freeze i1 %cmp
  br i1 %fcmp, label %if_then, label %if_else
if_then:
  call void @sink(i32 1)
  ret void
if_else:
  call void @sink(i32 2)
  ret void
}
```

Command:
```
opt -passes=jump-threading -S final_b.ll
```

Actual output:
```llvm
define void @test(i1 %c, i1 %maybe_poison) {
entry:
  br i1 %c, label %pred1, label %if_else

pred1:                                            ; preds = %entry
  br i1 %maybe_poison, label %if_then, label %if_else   ; <-- UB on poison

if_then:                                          ; preds = %pred1
  call void @sink(i32 1)
  ret void

if_else:                                          ; preds = %pred1, %entry
  call void @sink(i32 2)
  ret void
}
```

Diff vs expected: a `freeze i1 %maybe_poison` should be inserted before the
new `br i1` (or the transform should bail when
`isGuaranteedNotToBeUndefOrPoison` is false).

## Why this matters

This is a real miscompilation potential: any caller that passes a poison
i1 to a function shaped like the original code observes defined behavior
before the pass and undefined behavior after. Concretely, anything that
follows the load/freeze idiom (a very common pattern for taming
underdefined inputs) is at risk.

Modern alive2 disagreement: alive2 should report "target is more
poisonous" on this transform.

## Suggested fix

Mirror the freeze guard from `tryToUnfoldSelectInCurrBB`:

```cpp
// JumpThreading.cpp:2792 — replace
auto *BI = CondBrInst::Create(SI->getCondition(), NewBB, BB, Pred);
// with:
Value *Cond = SI->getCondition();
if (!isGuaranteedNotToBeUndefOrPoison(Cond, nullptr, SI))
  Cond = new FreezeInst(Cond, "cond.fr", SI->getIterator());
auto *BI = CondBrInst::Create(Cond, NewBB, BB, Pred);
```

The freeze is no-op when the condition is provably non-poison, so the
existing test snapshots that exercise unfoldSelectInstr with a defined
condition should remain unchanged.

## Related

- w260 / w261 — same routine drops `!unpredictable` on the synthesized
  branch (metadata bug).
- w671 (this) — same routine introduces UB on poison conditions
  (soundness bug). Independent of the metadata losses.
