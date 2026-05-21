# w250 — VectorCombine `widenSubvectorLoad` drops all metadata on widened load

## Files / locations

- `llvm/lib/Transforms/Vectorize/VectorCombine.cpp:406-412`
  Function: `VectorCombine::widenSubvectorLoad(Instruction &I)`

## Bug

`widenSubvectorLoad` rewrites:

```
%v = load <2 x float>, ptr %p, align 16, !nontemporal !0, !tbaa !1
%s = shufflevector <2 x float> %v, <2 x float> poison,
                   <4 x i32> <i32 0, i32 1, i32 poison, i32 poison>
```

into:

```
%s = load <4 x float>, ptr %p, align 16
```

The new wider `Builder.CreateAlignedLoad(Ty, CastedPtr, Alignment)` at line 409
is followed only by `replaceValue(I, *VecLd)` — there is **no
`copyMetadata`**, no `setAAMetadata`, no `setMetadata(MD_nontemporal, ...)`.
Every piece of memory-access metadata on the original `<2 x float>` load is
silently dropped:

- `!nontemporal` (perf, but also part of language reference)
- `!tbaa` (correctness — downstream AA depends on it for noalias proofs)
- `!alias.scope`, `!noalias` (correctness — alias info silently lost; the
  widened load may now be reordered against stores that the original was
  forbidden from aliasing)
- `!invariant.load`
- `!access_group`, `!mmra`

## Reproducer

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define <4 x float> @f(ptr align 16 dereferenceable(64) %p) {
  %v = load <2 x float>, ptr %p, align 16, !nontemporal !0, !tbaa !1
  %s = shufflevector <2 x float> %v, <2 x float> poison,
                     <4 x i32> <i32 0, i32 1, i32 poison, i32 poison>
  ret <4 x float> %s
}

!0 = !{i32 1}
!1 = !{!2, !2, i64 0}
!2 = !{!"float", !3, i64 0}
!3 = !{!"omnipotent char", !4, i64 0}
!4 = !{!"Simple C/C++ TBAA"}
```

`opt -passes='vector-combine' -S` produces:

```llvm
define <4 x float> @f(ptr align 16 dereferenceable(64) %p) {
  %s = load <4 x float>, ptr %p, align 16
  ret <4 x float> %s
}
```

`!nontemporal` and `!tbaa` are gone. `-O2 -S` also drops them.

## Why this is wrong

- Forwarding `!tbaa`/`!alias.scope`/`!noalias` to the wider load is
  conservatively correct for the dereferenceable region we already proved
  safe via `isSafeToLoadUnconditionally`. Dropping them throws away alias
  facts the downstream pipeline (instcombine, GVN, etc.) is allowed to use.
- Compare to the sibling routine `shrinkLoadForShuffles`
  (`VectorCombine.cpp:5650-5652`) which DOES call `NewLoad->copyMetadata(I)`.
  Forgetting it here is an inconsistency, not an intentional design.

## Fix sketch

Add after line 409:
```cpp
VecLd->copyMetadata(*Load);
```
(Or use the AAMetadata + selected-MD list pattern used in
`scalarizeLoadExtract`.)
