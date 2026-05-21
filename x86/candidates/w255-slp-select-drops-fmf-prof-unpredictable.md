# w255: SLPVectorizer `Instruction::Select` codegen drops FMF, `!prof`, and `!unpredictable`

## Pass
`-passes=slp-vectorizer` (default x86 -O2 pipeline includes this).

## Summary

`BoUpSLP::vectorizeTree` has a per-opcode switch in `vectorizeTree(TreeEntry *E, ...)`.
For most opcodes the result is annotated by calls to `propagateIRFlags` (copies
fast-math / wrap flags from the scalar bundle members) and
`::propagateMetadata` (intersects supported list metadata such as TBAA,
alias.scope, noalias, fpmath, access_group, nontemporal, etc.).

The `Instruction::Select` case (SLPVectorizer.cpp:22981-23022) calls **neither**.
It creates the vector select via `CreateSelectWithUnknownProfile`, applies any
necessary mask-shuffle, then immediately stores it in `E->VectorizedValue`. As a
result the vector select loses every flag/metadata that was on the scalar
selects:

- Fast-math flags (`nnan`, `ninf`, `nsz`, `arcp`, `contract`, `afn`, `reassoc`,
  `fast`) on FP selects.
- `!prof` branch-weight metadata (intentionally signalled by the use of
  `CreateSelectWithUnknownProfile`, but a better choice would be to copy the
  scalars' `!prof` when they agree).
- `!unpredictable` (codegen hint that suppresses `cmov` -> branch conversion in
  SelectionDAG).

For comparison, `Instruction::FNeg` immediately below (lines 23023-23039)
correctly does both `propagateIRFlags` and `::propagateMetadata`. The omission
in the Select case looks like an oversight (or a leftover from when `select`
did not carry FMF/metadata).

## Source (LLVM 23.0.0git, `llvm/lib/Transforms/Vectorize/SLPVectorizer.cpp`)

```cpp
// 22981 ─ case Instruction::Select: {
// ...
// 23015     Value *V =
// 23016         Builder.CreateSelectWithUnknownProfile(Cond, True, False, DEBUG_TYPE);
// 23017     V = FinalShuffle(V, E);
// 23018
// 23019     E->VectorizedValue = V;
// 23020     ++NumVectorInstructions;
// 23021     return V;
// 23022   }
// 23023   case Instruction::FNeg: {
// ...
// 23028     Value *V = Builder.CreateUnOp(...);
// 23029     // unlike Select, FNeg DOES propagate:
// 23030     propagateIRFlags(V, E->Scalars, VL0);
// 23031     if (auto *I = dyn_cast<Instruction>(V))
// 23032       V = ::propagateMetadata(I, E->Scalars);
```

There is no propagation between lines 22981 and 23022. (`grep -n
"propagateMetadata\|propagateIRFlags" SLPVectorizer.cpp` shows the closest
hits to 23015 are at 22970, 23030, 23032 — all outside the Select case.)

## Reproducer A — FMF dropped

`t_select_fmf.ll`:
```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @f(ptr %p, ptr %q, i1 %c) {
  %p0 = getelementptr float, ptr %p, i32 0
  %p1 = getelementptr float, ptr %p, i32 1
  %p2 = getelementptr float, ptr %p, i32 2
  %p3 = getelementptr float, ptr %p, i32 3
  %q0 = getelementptr float, ptr %q, i32 0
  %q1 = getelementptr float, ptr %q, i32 1
  %q2 = getelementptr float, ptr %q, i32 2
  %q3 = getelementptr float, ptr %q, i32 3
  %a  = load float, ptr %q0
  %b  = load float, ptr %q1
  %cc = load float, ptr %q2
  %d  = load float, ptr %q3
  %s0 = select nnan i1 %c, float %a,  float 1.0
  %s1 = select nnan i1 %c, float %b,  float 2.0
  %s2 = select nnan i1 %c, float %cc, float 3.0
  %s3 = select nnan i1 %c, float %d,  float 4.0
  store float %s0, ptr %p0
  store float %s1, ptr %p1
  store float %s2, ptr %p2
  store float %s3, ptr %p3
  ret void
}
```

Command:
```
opt -passes=slp-vectorizer -S t_select_fmf.ll
```

Output (relevant excerpt):
```llvm
  %1 = load <4 x float>, ptr %q0, align 4
  %2 = insertelement <4 x i1> poison, i1 %c, i64 0
  %3 = shufflevector <4 x i1> %2, <4 x i1> poison, <4 x i32> zeroinitializer
  %4 = select <4 x i1> %3, <4 x float> %1, <4 x float> <float 1.000000e+00, float 2.000000e+00, float 3.000000e+00, float 4.000000e+00>
  store <4 x float> %4, ptr %p0, align 4
```

Expected: `%4 = select nnan <4 x i1> ...` (every scalar had `nnan`, so the
intersection is `nnan`).

## Reproducer B — `!prof` dropped

Same shape, but selects carry `!prof`:
```llvm
  %s0 = select i1 %c, float %a,  float 1.0, !prof !1
  %s1 = select i1 %c, float %b,  float 2.0, !prof !1
  %s2 = select i1 %c, float %cc, float 3.0, !prof !1
  %s3 = select i1 %c, float %d,  float 4.0, !prof !1
  ; ...
!1 = !{!"branch_weights", i32 5, i32 7}
```

Output: the vectorized `%4 = select ...` has no `!prof` (the use of
`CreateSelectWithUnknownProfile` actively guarantees this even when all four
scalars carried the same metadata).

## Reproducer C — `!unpredictable` dropped

```llvm
  %s0 = select i1 %c, float %a,  float 1.0, !unpredictable !1
  %s1 = select i1 %c, float %b,  float 2.0, !unpredictable !1
  %s2 = select i1 %c, float %cc, float 3.0, !unpredictable !1
  %s3 = select i1 %c, float %d,  float 4.0, !unpredictable !1
!1 = !{}
```

Output: vector select has no `!unpredictable`. SelectionDAG then has no signal
to suppress `cmov` conversion for this vector select's eventual scalarization.

## Why this matters

- **FMF loss** is a real missed optimization: downstream passes can no longer
  use the `nnan/ninf/...` facts to constant-fold or simplify. For
  `Instruction::FNeg`, `Instruction::FAdd`, etc. SLP correctly propagates these
  flags; only `Select` is left out.
- **`!unpredictable` loss** changes back-end heuristics: the user explicitly
  marked the branch as not branch-predictor-friendly to discourage `cmov`
  expansion, and SLP silently strips that hint.
- **`!prof` loss** is signalled by the API name but still wrong — SLP knows the
  source profile (intersect across the bundle's selects, like LoadInst handles
  TBAA via `getMostGenericTBAA`).

## Suggested fix

Mirror the `FNeg` case after creating `V`:

```cpp
Value *V = Builder.CreateSelectFMFWithUnknownProfile(Cond, True, False,
                                                     /*FMFSource=*/VL0,
                                                     DEBUG_TYPE);
// or use CreateSelectWithUnknownProfile then:
propagateIRFlags(V, E->Scalars, VL0);
if (auto *I = dyn_cast<Instruction>(V))
  V = ::propagateMetadata(I, E->Scalars);
```

(`propagateMetadata` already supports the metadata kinds users put on select;
for `!prof` / `!unpredictable` an explicit copy from the bundle if all agree
would be straightforward and matches what other passes do.)
