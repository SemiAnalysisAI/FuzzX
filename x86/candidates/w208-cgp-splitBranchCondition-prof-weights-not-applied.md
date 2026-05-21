# w208: CodeGenPrepare::splitBranchCondition writes wrong branch-weight metadata (computed-but-unused locals)

**File:** `llvm/lib/CodeGen/CodeGenPrepare.cpp`
**Lines:** 9440-9491
**Function:** `CodeGenPrepare::splitBranchCondition`

## Summary
After splitting `br i1 (and/or %c1, %c2), TBB, FBB` into two consecutive branches `Br1`/`Br2`, CGP carefully computes new branch-weight pairs:
```c++
uint64_t NewTrueWeight = TrueWeight;
uint64_t NewFalseWeight = TrueWeight + 2 * FalseWeight;
scaleWeights(NewTrueWeight, NewFalseWeight);
Br1->setMetadata(LLVMContext::MD_prof,
                 MDBuilder(Br1->getContext())
                     .createBranchWeights(TrueWeight, FalseWeight,         // <-- BUG: original, unscaled
                                          hasBranchWeightOrigin(*Br1)));
```
`NewTrueWeight`/`NewFalseWeight` are computed and `scaleWeights`'d but never passed to `createBranchWeights`. The original `TrueWeight, FalseWeight` are passed verbatim. This bug is repeated **four times**: lines 9445-9448 (Or, Br1), 9453-9455 (Or, Br2), 9481-9483 (And, Br1), 9488-9490 (And, Br2).

The result: after `splitBranchCondition` runs, both Br1 and Br2 carry the **original**, **unmodified** weights from the wider conditional branch, rather than the recomputed weights that preserve overall path probabilities. The "redistribution" math is wasted. Downstream passes that consume profile data (block placement, BFI, machine outliner) see a wrong probability distribution.

## Source

```c++
// Or case (lines 9420-9456)
uint64_t TrueWeight, FalseWeight;
if (extractBranchWeights(*Br1, TrueWeight, FalseWeight)) {
  uint64_t NewTrueWeight = TrueWeight;
  uint64_t NewFalseWeight = TrueWeight + 2 * FalseWeight;
  scaleWeights(NewTrueWeight, NewFalseWeight);
  Br1->setMetadata(LLVMContext::MD_prof,
                   MDBuilder(Br1->getContext())
                       .createBranchWeights(TrueWeight, FalseWeight,
                                            hasBranchWeightOrigin(*Br1)));

  NewTrueWeight = TrueWeight;
  NewFalseWeight = 2 * FalseWeight;
  scaleWeights(NewTrueWeight, NewFalseWeight);
  Br2->setMetadata(LLVMContext::MD_prof,
                   MDBuilder(Br2->getContext())
                       .createBranchWeights(TrueWeight, FalseWeight));
}

// And case (lines 9476-9491) -- same bug, same pattern.
```

In all four places, the `New*Weight` locals are computed and `scaleWeights`'d but then ignored when calling `createBranchWeights`.

## Reproducer (`test_splitbranch.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @test(i32 %a, i32 %b, i32 %c) {
entry:
  %c1 = icmp slt i32 %a, %b
  %c2 = icmp sgt i32 %c, 0
  %cor = or i1 %c1, %c2
  br i1 %cor, label %taken, label %not_taken, !prof !0

taken:
  ret i32 1
not_taken:
  ret i32 0
}

!0 = !{!"branch_weights", i32 100, i32 1}
```

`splitBranchCondition` only runs when `EnableFastISel` is true, so use `-fast-isel`.

## Reproduce
```
$ /home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc \
    -mtriple=x86_64-unknown-linux-gnu -fast-isel \
    -stop-after=codegenprepare test_splitbranch.ll -o -
```

Observed IR after CGP:
```llvm
entry:
  %c1 = icmp slt i32 %a, %b
  br i1 %c1, label %taken, label %entry.cond.split, !prof !0       ; (100, 1)

entry.cond.split:                                 ; preds = %entry
  %c2 = icmp sgt i32 %c, 0
  br i1 %c2, label %taken, label %not_taken, !prof !0              ; (100, 1)
```

Both branches reuse `!prof !0 = {branch_weights, 100, 1}`. The original branch was strongly biased toward "taken". After splitting, the correct redistribution for an `or` should give Br1 a probability of `(A / (A + 2B))` for true, and Br2 a probability of `(A / (A + 2B))`. With A=100, B=1, the correct Br1 weights would be `(100, 102) -> scaled`, and Br2 weights `(100, 2) -> scaled`. Instead, both keep `(100, 1)`, which dramatically misrepresents the post-split probabilities.

## Suggested fix
Pass the scaled `NewTrueWeight`/`NewFalseWeight` instead of the originals to `createBranchWeights`:
```c++
uint64_t NewTrueWeight = TrueWeight;
uint64_t NewFalseWeight = TrueWeight + 2 * FalseWeight;
scaleWeights(NewTrueWeight, NewFalseWeight);
Br1->setMetadata(LLVMContext::MD_prof,
                 MDBuilder(Br1->getContext())
                     .createBranchWeights(NewTrueWeight, NewFalseWeight,
                                          hasBranchWeightOrigin(*Br1)));
```
Same fix for the other three sites.

## Impact
Wrong PGO-guided code layout, block placement, machine outliner, and any other pass that consumes branch-weight metadata. Code path that the source asserted is hot (via `!prof`) can be placed cold (and vice versa) after `splitBranchCondition` fires under `-fast-isel`. This is a long-standing, latent miscompile of profile metadata.

## Git history note
The author of `splitBranchCondition` clearly *intended* to apply the scaled weights — the locals exist for no other purpose, the comments explain the math, and `scaleWeights` is called. The bug looks like a copy-paste of variable names that was never noticed because nobody tests branch-weight values after CGP. A textbook "computed but unused" lint.
