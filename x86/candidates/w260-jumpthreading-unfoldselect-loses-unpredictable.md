# w260: JumpThreading `tryToUnfoldSelectInCurrBB` loses `!unpredictable` on the synthesized conditional branch

## Pass
`-passes=jump-threading` (default x86 -O2 pipeline includes this).

## Summary

`tryToUnfoldSelectInCurrBB` rewrites a `select` whose condition is an i1 PHI
(with at least one constant incoming) into a conditional branch via
`SplitBlockAndInsertIfThen`. The only metadata transferred from the original
`select` to the new branch is `MD_prof` (passed as `BranchWeights`). Any
`!unpredictable` metadata that was present on the `select` is silently
dropped from the new conditional branch.

`!unpredictable` is a CodeGen hint (suppresses `cmov`->`branch` conversion
in `SelectionDAGBuilder` and is consumed by `BranchProbabilityInfo`); losing
it changes the codegen heuristic the user explicitly asked for.

## Source (LLVM 23.0.0git, `llvm/lib/Transforms/Scalar/JumpThreading.cpp`)

```cpp
// line 2990
MDNode *BranchWeights = getBranchWeightMDNode(*SI);
Instruction *Term =
    SplitBlockAndInsertIfThen(Cond, SI, false, BranchWeights);
```

Only `MD_prof` is read off the `select` (`getBranchWeightMDNode`). There is
no `getMetadata(LLVMContext::MD_unpredictable)` or `Term->copyMetadata(...)`
call anywhere. Greppping the entire file confirms `MD_unpredictable` is
never referenced.

## Reproducer

Input `final_a.ll`:
```llvm
target triple = "x86_64-unknown-linux-gnu"
declare void @sink(i32)
define void @test(i1 %c0, i32 %x, i32 %y) {
entry:
  br i1 %c0, label %bb1, label %bb2
bb1:
  br label %merge
bb2:
  br label %merge
merge:
  %p = phi i1 [ true, %bb1 ], [ false, %bb2 ]
  %s = select i1 %p, i32 %x, i32 %y, !unpredictable !1
  call void @sink(i32 %s)
  ret void
}
!1 = !{}
```

Command:
```
opt -passes=jump-threading -S final_a.ll
```

Output (relevant excerpt):
```llvm
entry:
  br i1 %c0, label %0, label %merge        ; <-- no !unpredictable
```

Expected: the new conditional branch on `%c0` should carry the
`!unpredictable !1` that was on the original `select`. The select had it;
the branch is the semantic replacement.

## Why this matters

`!unpredictable` is consumed downstream:
- `BranchProbabilityInfo::calcUnpredictableHeuristics`
- `SelectionDAGBuilder::FindMergedConditions` uses it to decide between
  `cmov` and a real branch.

Without it the back-end may turn what the user marked as an unpredictable
branch into a `cmov`, which is the exact opposite of the intent of the
annotation. This is a metadata-correctness regression analogous to the
`!prof` loss bugs already tracked in tree.

## Suggested fix

After `SplitBlockAndInsertIfThen`, copy `MD_unpredictable` from `SI` onto
the new conditional branch that lives in `BB` (the split's head terminator
is `cast<BranchInst>(BB->getTerminator())`):

```cpp
if (MDNode *MD = SI->getMetadata(LLVMContext::MD_unpredictable))
  cast<BranchInst>(BB->getTerminator())->setMetadata(
      LLVMContext::MD_unpredictable, MD);
```

The same fix is needed in the sibling routine `unfoldSelectInstr` (see
w261).
