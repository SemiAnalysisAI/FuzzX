# w645 - SimplifyCFG `simplifyBranchOnICmpChain` integer-divides branch weights, annihilating profile

## Location

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp` lines 5304-5317
(`SimplifyCFGOpt::simplifyBranchOnICmpChain`).

The non-contiguous-values path that creates a new `SwitchInst` distributes
the original "hit-side" weight `BranchWeights[0]` across the N case arms
by **plain integer division**:

```cpp
} else {
  // Create the new switch instruction now.
  SwitchInst *New = Builder.CreateSwitch(CompVal, DefaultBB, Values.size());
  if (HasProfile) {
    // We know the weight of the default case. We don't know the weight of the
    // other cases, but rather than completely lose profiling info, we split
    // the remaining probability equally over them.
    SmallVector<uint32_t> NewWeights(Values.size() + 1);
    NewWeights[0] = BranchWeights[1]; // this is the default, and we swapped
                                      // if TrueWhenEqual.
    for (auto &V : drop_begin(NewWeights))
      V = BranchWeights[0] / Values.size();           // <<< INTEGER DIV
    setBranchWeights(*New, NewWeights, /*IsExpected=*/false);
  }
```

When `BranchWeights[0] < Values.size()`, every per-case weight becomes **0**,
so the entire hit-side probability is erased and the resulting switch tells
later passes "the default arm is the only reachable arm." Even when
`BranchWeights[0] >= Values.size()` the residual `BranchWeights[0] % N` is
silently discarded, biasing PGO toward `default`.

This path is reached from `SimplifyCFGOpt::simplifyCondBranch` (line 8585) with
no extra options required - the conversion is part of bare
`-passes=simplifycfg`, and the integer-division branch fires whenever the
input "hit" path has cold weight (e.g. raw PGO counts of 1, 2 or 3) compared
to the number of equality-tested constants.

## Repro (`repro.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define i32 @icmp_chain_to_switch(i32 %x) {
entry:
  %c0 = icmp eq i32 %x, 10
  %c1 = icmp eq i32 %x, 30
  %c2 = icmp eq i32 %x, 50
  %c3 = icmp eq i32 %x, 70
  %o0 = or i1 %c0, %c1
  %o1 = or i1 %c2, %c3
  %o2 = or i1 %o0, %o1
  br i1 %o2, label %hit, label %miss, !prof !0
hit:
  ret i32 1
miss:
  ret i32 0
}

; hit weight = 3, miss weight = 100. Four equality constants on the hit side.
!0 = !{!"branch_weights", i32 3, i32 100}
```

## Invocation

```
opt -passes=simplifycfg -S repro.ll
```

No extra switches needed; the integer-divide-by-N path is the default
non-contiguous-range fallback.

## Observed output

```
define i32 @icmp_chain_to_switch(i32 %x) {
entry:
  switch i32 %x, label %miss [
    i32 70, label %common.ret
    i32 50, label %common.ret
    i32 30, label %common.ret
    i32 10, label %common.ret
  ], !prof !0
  ...
}

!0 = !{!"branch_weights", i32 100, i32 0, i32 0, i32 0, i32 0}
```

Every case arm now has weight 0 (3 / 4 = 0). The "hit" probability of
~2.9% has been compressed to 0%. Downstream consumers (BFI, BPI, MIR
block placement, jump-table lowering heuristics) will treat the case
arms as unreachable-hot for placement and lay the code out as if the
default arm were the only viable destination. With a higher hit weight
(say {7, 100}) the bias is smaller but still wrong: 7 / 4 = 1, so the
emitted ratio is {100, 1, 1, 1, 1} (total 104, hit prob = 3.8%) instead
of the correct 6.5% — losing 43% of the hit signal.

## Why this is a profile-correctness bug, not just a heuristic

`!prof branch_weights` is documented in LangRef as an integer count, but
LLVM's middle-end and back-end use it to compute probabilities by
dividing each weight by the sum. The transformed switch now reports
`Pr(case=10) = 0/100 = 0`, which is a strictly different distribution
from the source `Pr(x==10 | x==30 | x==50 | x==70) = 3/103 ≈ 2.9% split
across the four cases`. Even an unbiased even split would emit
`{100, 1, 1, 1, 0}` (101 + leftover) or use `setFittedBranchWeights`
which rescales by a fitted denominator.

## Fix

Either:

1. Use `setFittedBranchWeights` (which the same function uses elsewhere)
   to rescale to a denominator that preserves the original ratio:

   ```cpp
   uint64_t Total = BranchWeights[0] + BranchWeights[1] * Values.size();
   SmallVector<uint64_t> NewWeights(Values.size() + 1);
   NewWeights[0] = BranchWeights[1] * Values.size();   // default keeps its mass
   for (auto &V : drop_begin(NewWeights))
     V = BranchWeights[0];                             // each arm gets full hit mass
   setFittedBranchWeights(*New, NewWeights, /*IsExpected=*/false);
   ```

   (Or better: scale both sides up by `Values.size()` so the ratio is
   preserved.)

2. Or distribute the remainder, e.g.:

   ```cpp
   uint32_t Per   = BranchWeights[0] / Values.size();
   uint32_t Extra = BranchWeights[0] % Values.size();
   for (size_t i = 1; i <= Values.size(); ++i)
     NewWeights[i] = Per + (i <= Extra ? 1 : 0);
   ```

   so weight is conserved.

The current code's comment ("rather than completely lose profiling info,
we split the remaining probability equally over them") is exactly what
the implementation fails to do for cold hit paths.
