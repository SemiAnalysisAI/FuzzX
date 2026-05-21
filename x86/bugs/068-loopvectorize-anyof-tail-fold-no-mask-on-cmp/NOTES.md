# w32: AnyOf reduction tail-folded loop OR-merges Cmp without masking inactive lanes (freeze yields non-zero in tail lanes)

## File
`llvm/lib/Transforms/Vectorize/LoopVectorize.cpp:7014-7059`
`llvm/lib/Transforms/Vectorize/VPlanRecipes.cpp:843-848`

## Pattern

LoopVectorize lowers an `AnyOf`-style reduction (scalar pattern:
`%sel = select %cmp, %newval, %phi`, where `%phi` is the running
"have we found one yet" value) by converting the reduction phi to
operate on `i1` and replacing the in-loop select with:

```cpp
// LoopVectorize.cpp:7036-7040
if (TrueValIsPhi)
  Cmp = Builder.createNot(Cmp);
VPValue *Or = Builder.createOr(PhiR, Cmp);     // <-- no HeaderMask AND
```

The freshly-created `Or` is `Or(PhiR, Cmp)` without AND-ing `Cmp`
with the loop's HeaderMask.  Then in the middle block:

```cpp
FinalReductionResult =
    Builder.createAnyOfReduction(NewExitingVPV, NewVal, Start, ExitDL);
```

The `AnyOf` VPInstruction is lowered (VPlanRecipes.cpp:843-848) as:

```cpp
case VPInstruction::AnyOf: {
  Value *Res = Builder.CreateFreeze(State.get(getOperand(0)));
  ...
  return State.VF.isScalar() ? Res : Builder.CreateOrReduce(Res);
}
```

A `freeze` on a `<VF x i1>` whose tail lanes are poison (because
`Cmp` was computed from a masked-load tail lane that returned
poison) may pick `true` for those lanes.  `Or(PhiR, Cmp)` had already
folded that poison into the i1 reduction phi.  After the
`OrReduce`, the answer is `true`, but the scalar loop would have
exited with `false` (no iteration actually satisfied the cmp).

## Why this is a miscompile, not deferred poison

The transform changes a scalar `select(cmp, ...)` (semantically
"did this iteration's `cmp` fire?") into `Or(prev, cmp_lane_i)`,
which folds *every* `Cmp` lane (including inactive ones) into the
reduction.  The original scalar loop never reads `cmp` for
out-of-bounds iterations; the vectorized loop now does, and a
single garbage lane suffices to flip the AnyOf answer.

The downstream `freeze` is intended to handle a *legitimate* one-
shot poison-source for early-exit reductions; here it *prevents*
the masked-load poison from being noticed and lets a poison bit be
chosen as true.

## Pre-conditions for triggering in x86 default -O2

- Loop with an AnyOf-style reduction pattern (e.g. "did any
  element equal X?").
- Tail-folded by masking (force with `-force-tail-folding-style=data`
  or with target where TTI prefers it, e.g. AArch64 with SVE; for
  x86 the path is reachable through `-prefer-predicate-over-epilogue`
  command-line override or `tail-folding` pragmas + AVX-512 masked
  loads).
- A load whose vector form is `masked.load` returning poison on
  inactive lanes.

## Suggested fix

After constructing `Or = Or(PhiR, Cmp)`, mask `Cmp` with HeaderMask:
`Cmp = And(Cmp, HeaderMask)` before the OR, *or* drop the freeze
in the AnyOf lowering and let the masked select-into-bool stay
poison-clean.  AArch64 tests around `select-cmp.ll` should be
re-checked once the AND is inserted.

## Repro plan

Manual repro via opt:
```
opt -passes=loop-vectorize -force-vector-width=4 \
    -force-tail-folding-style=data -S input.ll
```
On x86 the default cost model rejects predicated reduction unless
masked-load is "free" — `-mattr=+avx512f,+avx512vl` and a
non-noalias pointer that needs runtime aliasing checks can force
the tail-folded path (see existing test
`llvm/test/Transforms/LoopVectorize/X86/...select-cmp.ll`).

Confirmation would compare scalar vs vector output for a small
trip count (e.g. 7 elements, VF=4 ⇒ 1 vector iteration with 1
inactive lane) where the buffer's bytes past element 7 happen to
be a value that compares equal to the AnyOf target.

Status: code-inspection candidate; runtime repro not yet
constructed.
