# w267: FlattenCFG `MergeIfRegion` drops `!prof` from the first if-branch

## Summary

When `FlattenCFGOpt::MergeIfRegion` merges two adjacent if-regions
(`if (a) { ... } if (b) { ... }` -> `if (a || b) { ... }`), it splices the
second if-region's terminator into the first block after erasing the first
block's branch. The first branch's `!prof` metadata is lost; only the second
branch's `!prof` survives. The combined branch should carry a recomputed
weight reflecting both conditions.

## Source

- `llvm/lib/Transforms/Utils/FlattenCFG.cpp:475-478` —
  ```cpp
  FirstEntryBlock->back().eraseFromParent();   // drops first branch + its !prof
  FirstEntryBlock->splice(FirstEntryBlock->end(), SecondEntryBlock);
  CondBrInst *PBI = cast<CondBrInst>(FirstEntryBlock->getTerminator());
  ```
  `PBI` is the *second* branch with its original `!prof`. Then at
  `FlattenCFG.cpp:486-487`:
  ```cpp
  Value *NC = Builder.CreateBinOp(CombineOp, CInst1, PBI->getCondition());
  PBI->replaceUsesOfWith(PBI->getCondition(), NC);
  ```
  the condition is replaced with `cond1 OR/AND cond2` but `!prof` is never
  recomputed or even invalidated.

## Reproducer

`/home/orenamd@semianalysis.com/FuzzX/x86/candidates/w267-flattencfg-mergeifregion-drops-prof.ll`

```llvm
@g = external global i32

define void @test_then(i32 %x, i32 %y, i32 %z) {
entry.x:
  %cmp.x = icmp ne i32 %x, 0
  br i1 %cmp.x, label %if.then.x, label %entry.y, !prof !0  ; weights {1, 99}

if.then.x:
  store i32 %z, ptr @g, align 4
  br label %entry.y

entry.y:
  %cmp.y = icmp ne i32 %y, 0
  br i1 %cmp.y, label %if.then.y, label %exit, !prof !1     ; weights {50, 50}

if.then.y:
  store i32 %z, ptr @g, align 4
  br label %exit

exit:
  ret void
}
!0 = !{!"branch_weights", i32 1, i32 99}
!1 = !{!"branch_weights", i32 50, i32 50}
```

`opt -passes=flatten-cfg -S`:
```llvm
define void @test_then(i32 %x, i32 %y, i32 %z) {
entry.x:
  %cmp.x = icmp ne i32 %x, 0
  %cmp.y = icmp ne i32 %y, 0
  %0 = or i1 %cmp.x, %cmp.y
  br i1 %0, label %if.then.y, label %exit, !prof !0  ; only {50, 50} survives
  ...
}
!0 = !{!"branch_weights", i32 50, i32 50}
```

The first branch's `{1, 99}` profile is silently dropped. The correct combined
weight for `P(X || Y) = 1 - (1 - 0.01)*(1 - 0.5) = 0.505`, i.e. roughly
`{505, 495}`, not `{50, 50}`. (Aside: when `-O2` is run, this same input is
handled by SimplifyCFG instead, which computes `{4950, 5050}` — confirming
that the correct combined weight is computable; FlattenCFG simply doesn't do
it.)

## Caveats

`flatten-cfg` is not in the default x86 `-O2` pipeline (`--print-pipeline-passes
-O2` confirms). The bug is reproducible only when `flatten-cfg` is run
explicitly or via AMDGPU-style pipelines. PGO/AutoFDO downstream still relies
on `!prof` correctness, so this is a profile-fidelity bug rather than a
correctness miscompile.

## Fix sketch

Before erasing the first branch, read its `!prof`; compute combined weights
from both branches (e.g. via `scaleBranchWeights` / `combineBranchWeights`
helpers in `IR/ProfDataUtils.h`); set on the new combined branch. Same
treatment needed in `FlattenParallelAndOr` (see w268).
