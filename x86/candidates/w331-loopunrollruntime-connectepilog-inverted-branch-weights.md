# `LoopUnrollRuntime::ConnectEpilog` writes inverted branch weights for the epilog-guard branch

## File and root cause

`llvm/lib/Transforms/Utils/LoopUnrollRuntime.cpp` — `ConnectEpilog`, lines
362-385:

```cpp
// In NewExit, branch around the epilog loop if no extra iters.
Instruction *InsertPt = NewExit->getTerminator();
IRBuilder<> B(InsertPt);
Value *BrLoopExit = B.CreateIsNotNull(ModVal, "lcmp.mod");
assert(Exit && "Loop must have a single exit block only");
// Split the epilogue exit to maintain loop canonicalization guarantees
SmallVector<BasicBlock*, 4> Preds(predecessors(Exit));
SplitBlockPredecessors(Exit, Preds, ".epilog-lcssa", DT, LI, nullptr,
                       PreserveLCSSA);
// Add the branch to the exit block (around the epilog loop)
MDNode *BranchWeights = nullptr;
if (OriginalLoopProb.isUnknown() &&
    hasBranchWeightMD(*Latch->getTerminator())) {
  // Assume equal distribution in interval [0, Count).
  MDBuilder MDB(B.getContext());
  BranchWeights = MDB.createBranchWeights(1, Count - 1);          // <-- BUG
}
CondBrInst *RemainderLoopGuard =
    B.CreateCondBr(BrLoopExit, EpilogPreHeader, Exit, BranchWeights);
```

The semantics:

* `BrLoopExit = ICmpNE(ModVal, 0)`.
* `CondBr(BrLoopExit, EpilogPreHeader, Exit, BranchWeights)` — the **True**
  successor is `EpilogPreHeader` (taken when there are leftover iterations),
  the **False** successor is `Exit`.
* The comment says "Assume equal distribution in interval [0, Count)".  
  Under that assumption:
  * `P(ModVal == 0) = 1 / Count` (False branch),
  * `P(ModVal != 0) = (Count - 1) / Count` (True branch).
* `MDBuilder::createBranchWeights(uint32_t TrueWeight, uint32_t FalseWeight, ...)`
  takes the True weight first (see
  `include/llvm/IR/MDBuilder.h:80-82`).

So the True weight should be `Count - 1` and the False weight should be `1`.
The code passes them in the *reverse* order: `createBranchWeights(1, Count - 1)`
produces `{!"branch_weights", i32 1, i32 Count-1}`, telling downstream
analyses that taking the epilog is the *rare* path and skipping it the
common one. That is the exact opposite of the assumption stated in the
comment two lines above.

Cross-check the analogous prolog code in the same file at lines 165-190
(`ConnectProlog`):

```cpp
// "Assume loop is nearly always entered."
BranchWeights = MDB.createBranchWeights(UnrolledLoopHeaderWeights);   // {1, 127}
...
B.CreateCondBr(BrLoopExit, OriginalLoopLatchExit, NewPreHeader,
               BranchWeights);
```

There `BrLoopExit = ICmpULT(BECount, Count-1)` (True = skip the unrolled
loop, expected rare) and weights `{1, 127}` correctly put the small weight
on True. That call site is consistent; the epilog one is not.

## Trigger condition

The guarded branch only fires when:

1. `OriginalLoopProb.isUnknown()` — `getLoopProbability(L)` could not
   extract a probability (e.g. the latch is conditional but its terminator
   has malformed/incomplete `branch_weights` MD that `extractBranchWeights`
   rejects, or the latch is not actually exiting per
   `getExpectedExitLoopLatchBranch`), AND
2. `hasBranchWeightMD(*Latch->getTerminator())` — but the *raw* MD does
   exist on the terminator (`hasBranchWeightMD` does not require it to be
   parseable into a clean `BranchProbability`).

This is a narrow but real path: latch with a malformed `!prof` (e.g.
all-zero weights, or weights that fail the `extractBranchWeights` checks
post some other transform). The normal -O2 case where the latch has clean
weights goes through the `!OriginalLoopProb.isUnknown()` branch (lines
381-385) which uses `probOfNextInRemainder` and is unaffected.

## Reproducer

Below produces an epilog with the inverted-weight guard branch.  The
trigger is fragile to make portable in a single `.ll`, but the source bug
and inversion are visible from inspection alone: the comment and the
argument order disagree.

`/tmp/unroll-test/test-epil-prof.ll` — when the latch has clean weights
the code path falls through to the `OriginalLoopProb`-aware branch at
line 381 and is fine. The buggy line 377 is exercised only when
`OriginalLoopProb.isUnknown()` (`getLoopProbability` rejects the latch
weights) while `hasBranchWeightMD` is still true on the latch terminator.

## Why this is a regression

If this site fires, the epilog-guard branch records exactly inverted
probabilities. Block-frequency analyses, code layout, register allocation,
and any pass that consults `MD_prof` (e.g. `MachineBlockPlacement`) will
believe the epilog loop is the *cold* path when it is in fact taken with
probability `(Count - 1)/Count`. For a typical `Count = 8` epilog, this is
the difference between 12.5% cold and 87.5% hot — the layout and any cold
optimizations (`-Os`-style decisions, partial inlining, etc.) target the
wrong block.

## Fix

Swap the operands:

```cpp
BranchWeights = MDB.createBranchWeights(Count - 1, 1);
```

(Or equivalently use the named-array convention already adopted at the top
of this file: a constant `static const uint32_t EpilogGuardWeights[] = {Count-1, 1};`
expression — but since `Count` is dynamic at this call, the inline form
is fine.)
