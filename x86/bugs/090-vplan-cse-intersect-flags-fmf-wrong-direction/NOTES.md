# VPIRFlags::intersectFlags merges FMF in the wrong direction (and skips half the bits)

## Component
`llvm/lib/Transforms/Vectorize/VPlanRecipes.cpp` — `VPIRFlags::intersectFlags`
(consumer: `VPlanTransforms::cse` in `VPlanTransforms.cpp:2466-2490`)

## File / lines
- Bug site: `llvm/lib/Transforms/Vectorize/VPlanRecipes.cpp:343-391` (the
  `OperationType::FPMathOp` and `OperationType::ReductionOp` arms at lines 363-387)
- Caller: `llvm/lib/Transforms/Vectorize/VPlanTransforms.cpp:2477-2484`
- Reference: `llvm/include/llvm/IR/FMF.h:118-130` defines the canonical merge
  for FastMathFlags: `intersectRewrite` ANDs `AllowReassoc | AllowReciprocal |
  AllowContract | ApproxFunc`; `unionValue` ORs `NoNaNs | NoInfs |
  NoSignedZeros`.

## The bug
`VPIRFlags::intersectFlags` is called by VPlan CSE when collapsing a duplicate
`Def` into a kept recipe `V` (which dominates `Def`). It is meant to weaken
`V` to the flags that are valid for both recipes, since RAUW will redirect
`Def`'s users to `V`.

For the `FPMathOp` / `FCmp` case the implementation is:

```cpp
case OperationType::FPMathOp:
case OperationType::FCmp:
  ...
  getFMFsRef().NoNaNs &= Other.getFMFsRef().NoNaNs;
  getFMFsRef().NoInfs &= Other.getFMFsRef().NoInfs;
  break;
```

Two distinct problems compared to `FMF.h`'s canonical semantics:

1. **The "rewrite" permissions (AllowReassoc, AllowReciprocal, AllowContract,
   ApproxFunc) are never updated.** If `V` was authored with `reassoc` /
   `contract` / `arcp` / `afn` but `Def` was not, `V`'s flags are kept
   unchanged. Def's downstream users (which originally referenced a recipe
   without those rewrite permissions) now reference `V` and observe those
   permissions instead. Whether this enables a downstream reassociation /
   reciprocal / contraction / approximation that changes the numerical result
   depends on other ops in the chain having matching permissions, but the loss
   of the "noreassoc" invariant on Def's users' chains is exactly the kind of
   bug `intersectFlags` exists to prevent. Correct behavior is AND of these
   bits (`FastMathFlags::intersectRewrite`).

2. **`NoNaNs` and `NoInfs` are ANDed but should be ORed**, and `NoSignedZeros`
   is missing entirely. These are "value" flags: they assert that at runtime
   the operands/result are not NaN / not Inf / not -0. CSE has already proved
   `Def` and `V` compute the same value (same opcode, same operands, same
   type). If either asserted that no NaN/Inf/sign-zero arises, the runtime
   fact holds for the shared value, so the merged recipe is allowed to carry
   the union. `FMF.h`'s `unionValue` explicitly ORs `NoNaNs | NoInfs |
   NoSignedZeros`.
   Anding `NoNaNs/NoInfs` here is the opposite direction. It is conservatively
   *safe* (it only strips information V was entitled to keep), but it is the
   wrong-direction merge prescribed by LLVM semantics. The `NoSignedZeros`
   omission is also a missed opportunity rather than a correctness hazard.

Problem (1) is the actual miscompile vector. The `OperationType::ReductionOp`
arm at lines 378-387 has the identical bug (only NoNaNs/NoInfs touched; same
rewrite-vs-value confusion).

## Why this is reachable
`VPlanTransforms::cse` (VPlanTransforms.cpp:2466) calls
`RFlags->intersectFlags(*cast<VPRecipeWithIRFlags>(Def))` whenever two recipes
hash-equal and are isEqual (see `VPCSEDenseMapInfo::isEqual` at lines
2428-2460, which **does not consider FMF in equality** — so two `fadd`s with
different FMF will be merged here).

Minimal scenario:
- `%va = fadd reassoc <4 x float> %x, %y` (in a region where reassociation
  enables a later combine).
- A second `%vb = fadd <4 x float> %x, %y` (no reassoc) that flows into a
  user that intentionally relies on `(vb + a) + b` not being reassociated.
- CSE matches `va` and `vb` as equal, keeps `va`, calls
  `intersectFlags(*vb)`. Per the buggy code, `va` retains `reassoc`.
- `vb`'s downstream chain now sees `va` (reassoc). Subsequent VPlan or
  scalar-IR passes that look at FMF on the dominating fadd see permission to
  reassociate `(va + a) + b`, even though the author of `vb` did not grant it.

## Repro status
Static analysis only. I have not constructed a runtime miscompile. The bug is
structural in the merge routine and matches a well-defined invariant
(`FMF.h::intersectRewrite` / `unionValue`) that VPlan deviates from in
exactly two arms.

## Suggested fix
Replace the FPMathOp / ReductionOp arms with the canonical merge:

```cpp
case OperationType::FPMathOp:
case OperationType::FCmp: {
  FastMathFlags F = getFastMathFlags();
  FastMathFlags O = Other.getFastMathFlags();
  FastMathFlags Rewrite = FastMathFlags::intersectRewrite(F, O);
  FastMathFlags Value = FastMathFlags::unionValue(F, O);
  FastMathFlags Merged = Rewrite | Value;
  auto &Ref = getFMFsRef();
  Ref.AllowReassoc = Merged.allowReassoc();
  Ref.NoNaNs = Merged.noNaNs();
  Ref.NoInfs = Merged.noInfs();
  Ref.NoSignedZeros = Merged.noSignedZeros();
  Ref.AllowReciprocal = Merged.allowReciprocal();
  Ref.AllowContract = Merged.allowContract();
  Ref.ApproxFunc = Merged.approxFunc();
  break;
}
```
(and the same for `ReductionOp`).

## Tag
clang/llvm/Transforms/Vectorize/LoopVectorize — VPlan CSE, FMF merge.
