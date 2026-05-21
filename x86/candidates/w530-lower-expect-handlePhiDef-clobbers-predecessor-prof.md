# w530: LowerExpect handlePhiDef unconditionally clobbers predecessor PGO `!prof`

## Summary
When `handlePhiDef` propagates an `@llvm.expect` hint backward through a phi
to a dominating conditional branch in a **predecessor block**, it overwrites
that branch's existing `!prof` metadata without any check. Real PGO weights
on a completely separate condition are silently destroyed and replaced with
the synthetic 2000:1 expect weights.

## Source
File: `llvm/lib/Transforms/Scalar/LowerExpectIntrinsic.cpp`

```cpp
// lines 258-267
if (IsOpndComingFromSuccessor(BI->getSuccessor(1)))
  BI->setMetadata(LLVMContext::MD_prof,
                  MDB.createBranchWeights(LikelyBranchWeightVal,
                                          UnlikelyBranchWeightVal,
                                          /*IsExpected=*/true));
else if (IsOpndComingFromSuccessor(BI->getSuccessor(0)))
  BI->setMetadata(LLVMContext::MD_prof,
                  MDB.createBranchWeights(UnlikelyBranchWeightVal,
                                          LikelyBranchWeightVal,
                                          /*IsExpected=*/true));
```

`BI` is the **dominating conditional branch** of the phi's incoming block
(see `GetDomConditional`, lines 196-204). That branch is in a *predecessor*
to the merge block hosting the phi - it is decided by a condition that has
nothing to do with the expect call's argument. Yet `setMetadata` blows away
any pre-existing `!prof`.

A guard like `if (hasBranchWeightMD(*BI)) return;` or merging with the
existing weights (as e.g. `BlockFrequencyAnalysis` does) would respect prior
profile data. The helpers `llvm::hasBranchWeightMD` and
`llvm::getBranchWeightMDNode` already exist in `IR/ProfDataUtils.h`.

## Reproducer
```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @f(i1 %c, i32 %x) {
entry:
  br i1 %c, label %a, label %b, !prof !100   ; <-- real PGO data here
a:
  br label %merge
b:
  br label %merge
merge:
  %phi = phi i32 [ 1, %a ], [ 0, %b ]
  %e = call i32 @llvm.expect.i32(i32 %phi, i32 1)
  %cc = icmp ne i32 %e, 0
  br i1 %cc, label %t, label %f
t:
  ret i32 1
f:
  ret i32 2
}

declare i32 @llvm.expect.i32(i32, i32)
!100 = !{!"branch_weights", i32 8888, i32 7777}
```

Run:
```
opt -passes=lower-expect -S
```

## Observed diff (relevant lines)
Before (`%entry` block):
```
  br i1 %c, label %a, label %b, !prof !100
...
!100 = !{!"branch_weights", i32 8888, i32 7777}
```
After:
```
  br i1 %c, label %a, label %b, !prof !0
...
!0 = !{!"branch_weights", !"expected", i32 2000, i32 1}
```

The hand-tuned PGO weights `8888:7777` were replaced with the canonical
`LikelyBranchWeight:UnlikelyBranchWeight = 2000:1` of the expect lowering
even though the entry branch's condition is `%c`, not the value flowing
through the phi.

## Impact
Any frontend or pipeline that records `__builtin_expect()` *in addition to*
PGO data (e.g. `-fprofile-use` plus opaque-source headers that still carry
`__builtin_expect`) can have measured profile weights on entirely unrelated
predecessor branches replaced by synthetic ones. This skews
`BranchProbabilityInfo`, `BlockFrequencyInfo`, and any inliner / layout pass
that consumes them, producing worse codegen than no PGO at all on the
affected branches.

## Default-pipeline confirmation
`opt -passes=lower-expect` is invoked by the default x86 `-O2` pipeline (it
is registered as a `FunctionPass` early in `PassBuilderPipelines`). No
non-default flags required.
