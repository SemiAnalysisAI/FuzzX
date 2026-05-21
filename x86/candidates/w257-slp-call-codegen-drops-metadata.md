# w257: SLPVectorizer `Instruction::Call` codegen drops `!fpmath`/`!tbaa`/`!alias.scope`/`!access_group` on combined vector call

## Pass
`-passes=slp-vectorizer` (default x86 -O2 pipeline includes this).

## Summary

In `BoUpSLP::vectorizeTree`, the `Instruction::Call` case
(`SLPVectorizer.cpp` lines 23329-23403) creates a vector call (either a TLI
mapped vector function or an overloaded vector intrinsic such as
`llvm.sqrt.v4f32`), then calls `propagateIRFlags(V, E->Scalars, VL0)` only.
It never calls `::propagateMetadata`.

Compare to neighbouring binop/load/store cases which always do BOTH:

| Opcode | propagateIRFlags? | propagateMetadata? |
| --- | --- | --- |
| FNeg (23023-23039) | yes | yes |
| BinOp (23108-23122) | yes | yes |
| Load (23131-23244) | n/a | yes |
| Store (23247-23295) | n/a | yes |
| **Call (23329-23403)** | **yes** | **NO** |
| ShuffleVector (23405-23423) | yes | yes |

As a result every scalar-bundle merge of an intrinsic that carries metadata
(e.g. `!fpmath` on `llvm.sqrt`/`llvm.fma`, `!tbaa`/`!noalias` on memory-touching
intrinsics, `!llvm.access.group` on calls in parallel loops) loses that
metadata, even when every scalar in the bundle carried the same node.

## Source (LLVM 23.0.0git, `llvm/lib/Transforms/Vectorize/SLPVectorizer.cpp`)

```cpp
// 23329  case Instruction::Call: {
// ...
// 23395    Value *V = Builder.CreateCall(CF, OpVecs, OpBundles);
// 23396
// 23397    propagateIRFlags(V, E->Scalars, VL0);
// 23398    cast<CallInst>(V)->setCallingConv(CF->getCallingConv());
// 23399    V = FinalShuffle(V, E);
// 23400
// 23401    E->VectorizedValue = V;
// 23402    ++NumVectorInstructions;
// 23403    return V;
// 23404  }
```

There is no `::propagateMetadata(...)` call anywhere between 23395 (call
created) and 23403 (returned). All list metadata kinds supported by
`propagateMetadata` (TBAA, alias.scope, noalias, fpmath, nontemporal,
invariant_load, access_group, mmra) silently disappear.

## Reproducer — `!fpmath` dropped on combined `llvm.sqrt`

`t_call_md.ll`:
```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare float @llvm.sqrt.f32(float)

define void @f(ptr %p, ptr %q) {
  %p0 = getelementptr float, ptr %p, i32 0
  %p1 = getelementptr float, ptr %p, i32 1
  %p2 = getelementptr float, ptr %p, i32 2
  %p3 = getelementptr float, ptr %p, i32 3
  %q0 = getelementptr float, ptr %q, i32 0
  %q1 = getelementptr float, ptr %q, i32 1
  %q2 = getelementptr float, ptr %q, i32 2
  %q3 = getelementptr float, ptr %q, i32 3
  %a = load float, ptr %q0
  %b = load float, ptr %q1
  %c = load float, ptr %q2
  %d = load float, ptr %q3
  %sa = call float @llvm.sqrt.f32(float %a), !fpmath !1
  %sb = call float @llvm.sqrt.f32(float %b), !fpmath !1
  %sc = call float @llvm.sqrt.f32(float %c), !fpmath !1
  %sd = call float @llvm.sqrt.f32(float %d), !fpmath !1
  store float %sa, ptr %p0
  store float %sb, ptr %p1
  store float %sc, ptr %p2
  store float %sd, ptr %p3
  ret void
}

!1 = !{float 2.500000e+00}
```

Command:
```
opt -passes=slp-vectorizer -S t_call_md.ll
```

Output (relevant excerpt):
```llvm
  %1 = load <4 x float>, ptr %q0, align 4
  %2 = call <4 x float> @llvm.sqrt.v4f32(<4 x float> %1)   ; <-- no !fpmath !
  store <4 x float> %2, ptr %p0, align 4
```

Expected: `%2 = call <4 x float> @llvm.sqrt.v4f32(<4 x float> %1), !fpmath !0`
(every scalar had the same `!fpmath !1`, so the intersection via
`getMostGenericFPMath` is `!1`).

Control test (single scalar call):
```
opt -passes=slp-vectorizer -S t_call_single.ll
  %s = call float @llvm.sqrt.f32(float %a), !fpmath !0   ; preserved
```

A single sqrt is left alone and keeps `!fpmath`. The metadata loss is
specifically caused by SLP's vector-call construction path.

## Why this matters

- `!fpmath` is a fast-math precision hint consumed by InstCombine and the
  back-end. `BasicTTIImplBase::getInstructionCost` and
  `SelectionDAG::matchSqrtIEEE` use it to decide whether to emit `rsqrt`-style
  approximations. Losing the metadata reverts to the IEEE-exact path —
  semantically conservative but a real perf regression vs the user's
  declared tolerance.
- The same path drops `!tbaa`, `!alias.scope`, `!noalias`, `!access_group`
  for intrinsic calls that may touch memory (e.g. `llvm.masked.gather`,
  `llvm.memcpy.element.unordered.atomic` if they ever appeared as a
  vectorizable bundle).

## Suggested fix

Mirror the Binop / Load / Store / FNeg / ShuffleVector cases:

```cpp
Value *V = Builder.CreateCall(CF, OpVecs, OpBundles);

propagateIRFlags(V, E->Scalars, VL0);
cast<CallInst>(V)->setCallingConv(CF->getCallingConv());
+if (auto *I = dyn_cast<Instruction>(V))
+  V = ::propagateMetadata(I, E->Scalars);
V = FinalShuffle(V, E);
```

(`propagateMetadata` already intersects safely — `getMostGenericFPMath`,
`getMostGenericTBAA`, `intersect` for noalias/nontemporal/invariant_load, etc.)
