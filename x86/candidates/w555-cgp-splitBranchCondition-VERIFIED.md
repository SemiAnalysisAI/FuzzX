# w555 (VERIFIED): CodeGenPrepare::splitBranchCondition writes wrong branch-weight metadata (computed-but-unused locals)

**Status:** Real bug, reproduces. Source-of-truth review confirms the claim.
**Default-pipeline classification:** NON-DEFAULT — only reachable with explicit `-fast-isel` at `-O1/-O2/-O3`. Not exercised by `clang -O2`, `clang -O0`, `llc -O2`, or `llc -O0` (see "Reachability" below).
**Origin candidate:** `x86/candidates/w208-cgp-splitBranchCondition-prof-weights-not-applied.md`
**File:** `llvm/lib/CodeGen/CodeGenPrepare.cpp`
**Function:** `CodeGenPrepare::splitBranchCondition`
**Lines (LLVM head 10756d32f, dated 2026-05-16):** 9440-9491 (four call sites)
**Upstream status:** still present on `llvm/llvm-project` `main` as of this verification (curl-fetched 2026-05-21; identical text at lines 9440-9491).

## Summary

`splitBranchCondition` is the CodeGenPrepare equivalent of `SelectionDAGBuilder::FindMergedConditions`. It rewrites `br i1 (and/or %c1, %c2), TBB, FBB` into two sequential branches and is supposed to redistribute the original branch-weight metadata across them.

The author wrote the redistribution math correctly into locals (`NewTrueWeight`, `NewFalseWeight`), normalised them with `scaleWeights(...)`, and then **passed the un-recomputed originals `TrueWeight`/`FalseWeight` to `createBranchWeights`**. The scaled locals are dead. This happens four times (Or/Br1, Or/Br2, And/Br1, And/Br2).

Result: after `splitBranchCondition` fires, both resulting branches carry the original `!prof` of the wider branch verbatim. The redistribution math is wasted. Downstream consumers (BFI, MBP, MachineOutliner, hot/cold splitting) see wrong probabilities.

## Source (verified copy from local tree)

```c++
// Or case, lines 9440-9456
uint64_t TrueWeight, FalseWeight;
if (extractBranchWeights(*Br1, TrueWeight, FalseWeight)) {
  uint64_t NewTrueWeight = TrueWeight;
  uint64_t NewFalseWeight = TrueWeight + 2 * FalseWeight;
  scaleWeights(NewTrueWeight, NewFalseWeight);
  Br1->setMetadata(LLVMContext::MD_prof,
                   MDBuilder(Br1->getContext())
                       .createBranchWeights(TrueWeight, FalseWeight,        // BUG: should be NewTrueWeight, NewFalseWeight
                                            hasBranchWeightOrigin(*Br1)));

  NewTrueWeight = TrueWeight;
  NewFalseWeight = 2 * FalseWeight;
  scaleWeights(NewTrueWeight, NewFalseWeight);
  Br2->setMetadata(LLVMContext::MD_prof,
                   MDBuilder(Br2->getContext())
                       .createBranchWeights(TrueWeight, FalseWeight));      // BUG
}

// And case, lines 9476-9491 -- structurally identical, same bug
uint64_t TrueWeight, FalseWeight;
if (extractBranchWeights(*Br1, TrueWeight, FalseWeight)) {
  uint64_t NewTrueWeight = 2 * TrueWeight + FalseWeight;
  uint64_t NewFalseWeight = FalseWeight;
  scaleWeights(NewTrueWeight, NewFalseWeight);
  Br1->setMetadata(LLVMContext::MD_prof,
                   MDBuilder(Br1->getContext())
                       .createBranchWeights(TrueWeight, FalseWeight));      // BUG

  NewTrueWeight = 2 * TrueWeight;
  NewFalseWeight = FalseWeight;
  scaleWeights(NewTrueWeight, NewFalseWeight);
  Br2->setMetadata(LLVMContext::MD_prof,
                   MDBuilder(Br2->getContext())
                       .createBranchWeights(TrueWeight, FalseWeight));      // BUG
}
```

`scaleWeights` is defined at line 9285 and modifies its two by-reference args in place. The `NewTrueWeight`/`NewFalseWeight` locals are written, scaled, and then *never read*. They are computed-but-unused.

For contrast, `SelectionDAGBuilder::FindMergedConditions` (the function CGP's comment explicitly says it is mirroring) does pass the recomputed probabilities through to the recursive call (see `llvm/lib/CodeGen/SelectionDAG/SelectionDAGBuilder.cpp:2717-2728, 2750-2761`).

## Gating

```c++
// CodeGenPrepare.cpp:9314
bool CodeGenPrepare::splitBranchCondition(Function &F) {
  if (!TM->Options.EnableFastISel || TLI->isJumpExpensive())
    return false;
  ...
```

So this code only executes when both:
1. `TM->Options.EnableFastISel == true`, AND
2. `TLI->isJumpExpensive() == false` (true for X86 by default).

## Reachability (verified by direct llc experiment)

| Invocation                       | CGP runs?           | `EnableFastISel` at CGP-run time | `splitBranchCondition` fires? | Bug reachable?             |
| -------------------------------- | ------------------- | -------------------------------- | ----------------------------- | -------------------------- |
| `llc -O2` (default)              | yes                 | false                            | no                            | NO                         |
| `llc -O0` (default)              | NO (gated off at O0; TPC `addCodeGenPrepare()` line 959 requires `OptLevel != None`) | n/a              | no                            | NO                         |
| `llc -O0 -fast-isel`             | NO (same reason)    | n/a                              | no                            | NO                         |
| `llc -O2 -fast-isel`             | yes                 | true                             | yes                           | **YES**                    |
| `clang -O2` (default)            | yes                 | false                            | no                            | NO                         |
| `clang -O0` (default)            | NO                  | n/a                              | no                            | NO                         |
| `clang -O2 -mllvm -fast-isel`    | yes                 | true                             | yes                           | **YES** (rare config)      |

Why `clang -O0` does not reach this code: at `-O0` clang/llc takes the "no-CGP" path because `TargetPassConfig::addCodeGenPrepare()` adds the CGP pass only when `getOptLevel() != CodeGenOptLevel::None`. Fast-isel selection (where `Options.EnableFastISel` is finally flipped to true via `TM->setFastISel(true)` in `addCoreISelPasses`, line 1011) happens regardless, but the IR-level CGP pass is simply not part of the `-O0` pipeline.

So the only realistic way to hit this bug is the unusual combination of an optimised build AND an explicit `-fast-isel` override. That is rarely used outside of LLVM developers debugging fast-isel coverage. Not a default-pipeline x86 -O2 bug.

## Reproducer

`/tmp/test_splitbranch.ll`:

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

Command:

```
/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc \
    -mtriple=x86_64-unknown-linux-gnu -O2 -fast-isel \
    -stop-after=codegenprepare /tmp/test_splitbranch.ll -o -
```

Observed output (relevant slice):

```llvm
entry:
  %c1 = icmp slt i32 %a, %b
  br i1 %c1, label %taken, label %entry.cond.split, !prof !0     ; (100, 1)

entry.cond.split:                                 ; preds = %entry
  %c2 = icmp sgt i32 %c, 0
  br i1 %c2, label %taken, label %not_taken, !prof !0            ; (100, 1)
```

Both new branches inherit the original `!0 = {branch_weights, 100, 1}` verbatim. For the `or` case with A=100, B=1, the intended distribution (per the in-source comment) is:

- Br1: TrueProb = A / (A+2B) = 100/102; weights (NewTrueWeight, NewFalseWeight) = (100, 102) before scaling.
- Br2: TrueProb = A / (A+2B); weights (NewTrueWeight, NewFalseWeight) = (100, 2) before scaling.

Instead both get (100, 1), which says "TBB is taken 99% of the time on each independent branch," giving an aggregate TBB probability of ~0.9999 vs the original 100/101 ~= 0.9901. Other (A,B) combinations diverge more dramatically: with weights `(1, 100)` (cold-but-fall-through) the original branch path-probability is 1/101 ~= 0.0099; after the broken split each leg keeps `(1,100)` so the aggregate TBB probability becomes `1/101 + (100/101)(1/101)` ~= 0.0197 — almost double.

Also reproducible with the `and` form by replacing `or i1` with `and i1` in the .ll; same wrong-weights pattern emerges.

### Negative controls (verified locally)

```
$ llc -mtriple=x86_64-unknown-linux-gnu -stop-after=codegenprepare /tmp/test_splitbranch.ll -o -        # no -fast-isel: branch NOT split, original (100,1) preserved on single br
$ llc -mtriple=x86_64-unknown-linux-gnu -O2 -stop-after=codegenprepare /tmp/test_splitbranch.ll -o -     # -O2 default: not split
$ llc -mtriple=x86_64-unknown-linux-gnu -O0 -stop-after=codegenprepare /tmp/test_splitbranch.ll -o -     # -O0: CGP pass not in pipeline, no IR dump emitted
$ llc -mtriple=x86_64-unknown-linux-gnu -O0 -fast-isel -stop-after=codegenprepare ...                    # -O0+fast-isel: still no CGP, no split
```

## Fix

Pass the scaled `NewTrueWeight`/`NewFalseWeight` instead of the originals at all four call sites:

```c++
.createBranchWeights(NewTrueWeight, NewFalseWeight, hasBranchWeightOrigin(*Br1))
```

(for the Br1 site) and likewise for Br2 / both And sites.

## Impact

Wrong PGO-guided code layout for any function whose source goes through `clang -O2 -mllvm -fast-isel` (or equivalent) and contains short-circuited boolean conditions with `!prof` metadata. Affects BlockFrequencyInfo, MachineBlockPlacement, hot/cold splitting, MachineOutliner. Latent miscompile of *profile metadata*, not of program semantics — no observable wrong answers, only suboptimal placement.

Because the `clang -O2 -mllvm -fast-isel` combination is uncommon in production builds (fast-isel is a debug aid; people who use it generally do so at `-O0`, where CGP doesn't run), the *practical* blast radius is small. The code itself is unambiguously wrong though.

## Why the bug looks like a copy-paste oversight

- The `NewTrueWeight`/`NewFalseWeight` locals exist for no other purpose.
- They are computed and `scaleWeights`'d in a way that is *only* meaningful as input to `createBranchWeights`.
- The in-source comment block ("Assuming the original weights are A and B, one choice is to set BB1's weights to A and A+2B...") explicitly describes the intended scaled values.
- `SelectionDAGBuilder::FindMergedConditions` (the function CGP's comment says it mirrors) correctly uses the scaled probabilities.

So `Options.EnableFastISel` + the lack of any test that inspects post-CGP branch-weight metadata together hide the bug. A trivial unused-variable lint would have caught it — `clang-tidy bugprone-unused-local-non-trivial-variable` does not flag `uint64_t`, but a stronger "computed-but-unused" diagnostic would.
